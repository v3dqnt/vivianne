//! Semantic code search: a per-project embedding index plus BM25, combined.
//!
//! `code_search` used to be keyword overlap that called itself semantic — a
//! query for "function that sums two numbers" found a README before the
//! `add()` it described. This module is the real thing:
//!
//! - **Chunks.** Each source file is cut into overlapping windows of lines,
//!   preferring to start a window at a blank line or a definition so a
//!   function tends to land whole in one chunk.
//! - **Embeddings.** Chunks are embedded with the first provider the user has
//!   a key for — OpenAI `text-embedding-3-small`, or NVIDIA's free
//!   `nv-embedqa-e5-v5` — and kept in `~/.viv/index/<project>.json`. Only
//!   files whose size or modification time changed are embedded again.
//! - **Hybrid ranking.** Cosine similarity finds code that *means* the query;
//!   BM25 over identifier-split tokens (`parseConfig` → `parse`, `config`)
//!   keeps exact names ranking first when the query names them. The two are
//!   blended after normalising each to 0..1.
//!
//! With no embedding key, search still works: BM25 alone, which already beats
//! the old overlap count because it weighs rare words and splits identifiers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const CHUNK_LINES: usize = 40;
const CHUNK_OVERLAP: usize = 8;
const MAX_FILES: usize = 4000;
const MAX_FILE_BYTES: u64 = 512_000;
const MAX_CHUNKS: usize = 20_000;
const EMBED_BATCH: usize = 64;
/// First-time indexing of a big project can take a while; past this the
/// search answers from what's indexed so far (and says so) rather than
/// holding the turn hostage. The rest is picked up by the next search.
const INDEX_TIME_BUDGET: Duration = Duration::from_secs(60);
/// Weight of semantic similarity against BM25 in the blended score.
const SEMANTIC_WEIGHT: f32 = 0.7;

pub const SEARCHABLE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "swift", "c", "cc", "cpp", "h", "hpp", "cs",
    "rb", "php", "scala", "lua", "dart", "vue", "svelte", "toml", "yaml", "yml", "json", "md", "css", "scss", "html",
    "sql", "sh", "ps1",
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Embedder {
    OpenAi,
    Nvidia,
    /// The bundled offline all-MiniLM model, when it's been downloaded.
    Local,
    /// The offline EmbeddingGemma model, when it's been downloaded.
    LocalGemma,
}

impl Embedder {
    fn model(self) -> &'static str {
        match self {
            Embedder::OpenAi => "text-embedding-3-small",
            Embedder::Nvidia => "nvidia/nv-embedqa-e5-v5",
            Embedder::Local => "all-MiniLM-L6-v2",
            Embedder::LocalGemma => "embeddinggemma-300m",
        }
    }
    fn url(self) -> &'static str {
        match self {
            Embedder::OpenAi => "https://api.openai.com/v1/embeddings",
            Embedder::Nvidia => "https://integrate.api.nvidia.com/v1/embeddings",
            Embedder::Local | Embedder::LocalGemma => "",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Embedder::OpenAi => "OpenAI embeddings",
            Embedder::Nvidia => "NVIDIA embeddings",
            Embedder::Local => "local MiniLM embeddings",
            Embedder::LocalGemma => "local EmbeddingGemma embeddings",
        }
    }
}

/// The embedding provider to use and its key: OpenAI first (fast, cheap,
/// strong on code), then NVIDIA (free), then the offline MiniLM model if it's
/// installed (weaker on code, but no key and no network). `None` = keyword
/// search only.
pub async fn pick_embedder() -> Option<(Embedder, String)> {
    for (e, store_ids, env) in [
        (Embedder::OpenAi, &["openai"][..], "OPENAI_API_KEY"),
        (Embedder::Nvidia, &["nvidia", "nvidia-nim"][..], "NVIDIA_API_KEY"),
    ] {
        for id in store_ids {
            if let Some(k) = crate::ai::auth::stored_api_key(id).await {
                return Some((e, k));
            }
        }
        if let Ok(k) = std::env::var(env) {
            if !k.trim().is_empty() {
                return Some((e, k));
            }
        }
    }
    match crate::ai::local_onnx::active() {
        Some(crate::ai::local_onnx::LocalModel::Gemma) => return Some((Embedder::LocalGemma, String::new())),
        Some(crate::ai::local_onnx::LocalModel::MiniLm) => return Some((Embedder::Local, String::new())),
        None => {}
    }
    None
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(default)]
    pub vector: Vec<f32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    embedder: Option<Embedder>,
    /// path → (size, mtime secs), to spot changed files.
    files: HashMap<String, (u64, u64)>,
    chunks: Vec<Chunk>,
}

fn index_path(root: &Path) -> Option<PathBuf> {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&canon.to_string_lossy().to_lowercase(), &mut hasher);
    let id = std::hash::Hasher::finish(&hasher);
    let dir = crate::agent::system::paths::viv_home().ok()?.join("index");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{id:016x}.json")))
}

// ── Tokens and BM25 ───────────────────────────────────────────────────────

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "of", "to", "in", "on", "for", "and", "or", "do", "does", "where", "what", "how",
    "find", "code", "that", "this", "with", "we", "it", "be", "by", "as", "at", "from", "which", "function", "file",
];

/// Split text into search tokens: lower-case words, with identifiers broken at
/// case changes and underscores (`parseHTTPConfig` → parse, http, config) and
/// a light plural/verb stem so `sums` meets `sum`.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let mut word = String::new();
        let chars: Vec<char> = raw.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let boundary = i > 0
                && c.is_uppercase()
                && (chars[i - 1].is_lowercase() || chars.get(i + 1).is_some_and(|n| n.is_lowercase()) && chars[i - 1].is_uppercase());
            if boundary && !word.is_empty() {
                push_token(&mut out, &word);
                word.clear();
            }
            word.push(c);
        }
        push_token(&mut out, &word);
    }
    out
}

fn push_token(out: &mut Vec<String>, word: &str) {
    let w = word.to_lowercase();
    if w.len() < 2 || STOPWORDS.contains(&w.as_str()) || w.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    out.push(stem(&w));
}

fn stem(w: &str) -> String {
    for suffix in ["ing", "ies", "es", "ed", "s"] {
        if w.len() > suffix.len() + 2 && w.ends_with(suffix) {
            let base = &w[..w.len() - suffix.len()];
            return if suffix == "ies" { format!("{base}y") } else { base.to_string() };
        }
    }
    w.to_string()
}

fn bm25_scores(query: &[String], chunks: &[Chunk]) -> Vec<f32> {
    const K1: f32 = 1.2;
    const B: f32 = 0.75;
    let docs: Vec<Vec<String>> = chunks.iter().map(|c| tokens(&format!("{} {}", c.path, c.text))).collect();
    let n = docs.len().max(1) as f32;
    let avg_len = docs.iter().map(|d| d.len()).sum::<usize>() as f32 / n;
    let mut df: HashMap<&str, f32> = HashMap::new();
    for q in query {
        let count = docs.iter().filter(|d| d.iter().any(|t| t == q)).count() as f32;
        df.insert(q.as_str(), count);
    }
    docs.iter()
        .map(|d| {
            let len = d.len() as f32;
            query
                .iter()
                .map(|q| {
                    let tf = d.iter().filter(|t| *t == q).count() as f32;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let dfq = df[q.as_str()];
                    let idf = ((n - dfq + 0.5) / (dfq + 0.5) + 1.0).ln();
                    idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * len / avg_len.max(1.0)))
                })
                .sum()
        })
        .collect()
}

// ── Chunking ──────────────────────────────────────────────────────────────

fn looks_like_definition(line: &str) -> bool {
    let t = line.trim_start();
    ["fn ", "pub fn", "async fn", "def ", "class ", "function ", "export ", "const ", "impl ", "struct ", "interface ", "type ", "func "]
        .iter()
        .any(|p| t.starts_with(p))
}

pub fn chunk_file(path: &str, content: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = (start + CHUNK_LINES).min(lines.len());
        // End at a natural boundary if one is near, so a definition isn't split.
        if end < lines.len() {
            if let Some(b) = (end.saturating_sub(10)..end).rev().find(|&i| lines[i].trim().is_empty() || looks_like_definition(lines[i])) {
                if b > start + CHUNK_LINES / 2 {
                    end = b;
                }
            }
        }
        let text = lines[start..end].join("\n");
        if !text.trim().is_empty() {
            chunks.push(Chunk { path: path.to_string(), start: start + 1, end, text, vector: Vec::new() });
        }
        if end >= lines.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP).max(start + 1);
    }
    chunks
}

// ── Embeddings ────────────────────────────────────────────────────────────

fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap_or_else(|_| reqwest::Client::new()))
}

async fn embed(embedder: Embedder, key: &str, inputs: &[String], query: bool) -> Result<Vec<Vec<f32>>, String> {
    if matches!(embedder, Embedder::Local | Embedder::LocalGemma) {
        let purpose = if query { crate::ai::local_onnx::Purpose::Query } else { crate::ai::local_onnx::Purpose::Document };
        return match crate::ai::local_onnx::embed_local_batch(inputs, purpose).await? {
            Some(vectors) => Ok(vectors.into_iter().map(normalise).collect()),
            None => Err("the local embedding model isn't available".to_string()),
        };
    }
    let mut body = serde_json::json!({ "model": embedder.model(), "input": inputs, "encoding_format": "float" });
    if embedder == Embedder::Nvidia {
        body["input_type"] = serde_json::json!(if query { "query" } else { "passage" });
        body["truncate"] = serde_json::json!("END");
    }
    let resp = http().post(embedder.url()).bearer_auth(key).json(&body).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("HTTP {status}: {e}"))?;
    if !status.is_success() {
        let msg = json.pointer("/error/message").or_else(|| json.get("detail")).map(|m| m.to_string()).unwrap_or_default();
        return Err(format!("HTTP {} {msg}", status.as_u16()));
    }
    let data = json.get("data").and_then(|d| d.as_array()).ok_or("no embeddings in the response")?;
    let mut rows: Vec<(usize, Vec<f32>)> = data
        .iter()
        .map(|row| {
            let i = row.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            let v = row
                .get("embedding")
                .and_then(|e| e.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect())
                .unwrap_or_default();
            (i, v)
        })
        .collect();
    rows.sort_by_key(|(i, _)| *i);
    Ok(rows.into_iter().map(|(_, v)| normalise(v)).collect())
}

fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Text sent to the embedder: the path helps (a file called `auth.rs` is
/// about auth), and the cap keeps a long chunk inside the model's limit.
fn embed_text(c: &Chunk) -> String {
    let body: String = c.text.chars().take(2400).collect();
    format!("{}\n{}", c.path, body)
}

// ── Indexing ──────────────────────────────────────────────────────────────

pub struct Status {
    pub embedder: Option<Embedder>,
    pub files: usize,
    pub chunks: usize,
    pub newly_embedded: usize,
    pub incomplete: bool,
    pub embed_error: Option<String>,
}

/// Bring the project's index up to date and return it with a status.
async fn refresh(root: &Path, embedder: Option<(Embedder, String)>) -> (Index, Status) {
    let path = index_path(root);
    let mut index: Index = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let which = embedder.as_ref().map(|(e, _)| *e);
    if index.embedder != which {
        // Vectors from another model aren't comparable; start over.
        index = Index { embedder: which, ..Default::default() };
    }

    // Walk the tree (gitignore-aware, same walker grep uses).
    let root_buf = root.to_path_buf();
    let seen: Vec<(String, u64, u64, PathBuf)> = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        super::tool_exec::walk_files(&root_buf, MAX_FILES, |p| {
            let ext_ok = p.extension().and_then(|e| e.to_str()).is_some_and(|e| SEARCHABLE_EXTS.contains(&e.to_ascii_lowercase().as_str()));
            if !ext_ok {
                return true;
            }
            if let Ok(meta) = std::fs::metadata(p) {
                if meta.len() <= MAX_FILE_BYTES {
                    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                    let rel = p.strip_prefix(&root_buf).unwrap_or(p).to_string_lossy().replace('\\', "/");
                    out.push((rel, meta.len(), mtime, p.to_path_buf()));
                }
            }
            true
        });
        out
    })
    .await
    .unwrap_or_default();

    let present: std::collections::HashSet<&str> = seen.iter().map(|(r, ..)| r.as_str()).collect();
    index.chunks.retain(|c| present.contains(c.path.as_str()));
    index.files.retain(|p, _| present.contains(p.as_str()));

    for (rel, size, mtime, full) in &seen {
        if index.files.get(rel) == Some(&(*size, *mtime)) {
            continue;
        }
        index.chunks.retain(|c| &c.path != rel);
        if let Ok(content) = std::fs::read_to_string(full) {
            index.chunks.extend(chunk_file(rel, &content));
        }
        index.files.insert(rel.clone(), (*size, *mtime));
        if index.chunks.len() > MAX_CHUNKS {
            break;
        }
    }
    index.chunks.truncate(MAX_CHUNKS);

    let mut status = Status {
        embedder: which,
        files: index.files.len(),
        chunks: index.chunks.len(),
        newly_embedded: 0,
        incomplete: false,
        embed_error: None,
    };

    if let Some((e, key)) = embedder {
        let started = std::time::Instant::now();
        let todo: Vec<usize> = (0..index.chunks.len()).filter(|&i| index.chunks[i].vector.is_empty()).collect();
        for batch in todo.chunks(EMBED_BATCH) {
            if started.elapsed() > INDEX_TIME_BUDGET {
                status.incomplete = true;
                break;
            }
            let inputs: Vec<String> = batch.iter().map(|&i| embed_text(&index.chunks[i])).collect();
            match embed(e, &key, &inputs, false).await {
                Ok(vectors) if vectors.len() == batch.len() => {
                    for (&i, v) in batch.iter().zip(vectors) {
                        index.chunks[i].vector = v;
                    }
                    status.newly_embedded += batch.len();
                }
                Ok(_) => {
                    status.embed_error = Some("the embedder returned the wrong number of vectors".into());
                    break;
                }
                Err(err) => {
                    status.embed_error = Some(err);
                    break;
                }
            }
        }
    }

    if let Some(p) = path {
        if let Ok(json) = serde_json::to_string(&index) {
            let _ = crate::agent::runtime::commands::write_session_file_atomic(&p, &json);
        }
    }
    (index, status)
}

pub struct Hit {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub score: f32,
    pub preview: String,
}

/// Search `root` for `query`. Returns the best chunks and how the search ran.
pub async fn search(root: &Path, query: &str, limit: usize) -> (Vec<Hit>, Status) {
    let embedder = pick_embedder().await;
    let (index, mut status) = refresh(root, embedder.clone()).await;
    let q_tokens = tokens(query);
    let lexical = bm25_scores(&q_tokens, &index.chunks);
    let max_lex = lexical.iter().cloned().fold(0.0f32, f32::max);

    let mut semantic = vec![0.0f32; index.chunks.len()];
    let mut have_semantic = false;
    if let (Some((e, key)), None) = (&embedder, &status.embed_error) {
        match embed(*e, key, &[query.to_string()], true).await {
            Ok(mut v) if !v.is_empty() => {
                let qv = v.remove(0);
                for (i, c) in index.chunks.iter().enumerate() {
                    if !c.vector.is_empty() {
                        semantic[i] = dot(&qv, &c.vector);
                        have_semantic = true;
                    }
                }
            }
            Ok(_) => {}
            Err(err) => status.embed_error = Some(err),
        }
    }
    let (min_sem, max_sem) = semantic
        .iter()
        .zip(&index.chunks)
        .filter(|(_, c)| !c.vector.is_empty())
        .fold((f32::MAX, f32::MIN), |(lo, hi), (s, _)| (lo.min(*s), hi.max(*s)));

    let mut scored: Vec<(f32, usize)> = (0..index.chunks.len())
        .map(|i| {
            let lex = if max_lex > 0.0 { lexical[i] / max_lex } else { 0.0 };
            let sem = if have_semantic && !index.chunks[i].vector.is_empty() && max_sem > min_sem {
                (semantic[i] - min_sem) / (max_sem - min_sem)
            } else {
                0.0
            };
            let score = if have_semantic { SEMANTIC_WEIGHT * sem + (1.0 - SEMANTIC_WEIGHT) * lex } else { lex };
            (score, i)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // One result per file region: overlapping chunks of the same file would
    // otherwise crowd out everything else.
    let mut hits: Vec<Hit> = Vec::new();
    for (score, i) in scored {
        let c = &index.chunks[i];
        if hits.iter().any(|h| h.path == c.path && c.start <= h.end && h.start <= c.end) {
            continue;
        }
        let preview = c.text.lines().filter(|l| !l.trim().is_empty()).take(6).collect::<Vec<_>>().join("\n");
        hits.push(Hit { path: c.path.clone(), start: c.start, end: c.end, score, preview });
        if hits.len() >= limit {
            break;
        }
    }
    if !have_semantic && status.embedder.is_some() && status.embed_error.is_none() {
        status.embedder = None;
    }
    (hits, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_split_and_stem() {
        assert_eq!(tokens("parseHTTPConfig"), vec!["parse", "http", "config"]);
        assert_eq!(tokens("sum_numbers"), vec!["sum", "number"]);
        assert_eq!(tokens("the sums of all entries"), vec!["sum", "all", "entry"]);
    }

    #[test]
    fn chunks_cover_the_file_with_overlap() {
        let content: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        let chunks = chunk_file("a.rs", &content);
        assert!(chunks.len() >= 3);
        assert_eq!(chunks[0].start, 1);
        assert_eq!(chunks.last().unwrap().end, 100);
        for w in chunks.windows(2) {
            assert!(w[1].start <= w[0].end, "gap between chunks");
        }
    }

    #[test]
    fn bm25_prefers_the_rare_matching_word() {
        let mk = |p: &str, t: &str| Chunk { path: p.into(), start: 1, end: 1, text: t.into(), vector: vec![] };
        let chunks = vec![mk("a.rs", "fn render() {}"), mk("b.rs", "fn parse_config() {} fn render() {}"), mk("c.rs", "fn render() {}")];
        let s = bm25_scores(&tokens("parse config"), &chunks);
        assert!(s[1] > s[0] && s[1] > s[2]);
    }

    /// Live: indexes a small project with the user's real embedding key and
    /// checks that meaning, not wording, finds the code.
    /// `cargo test --lib semantic_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn semantic_live() {
        if crate::ai::keychain_store::KeychainCredentialStore::is_available() {
            crate::ai::auth::set_global(std::sync::Arc::new(crate::ai::auth::AuthState::with_keychain_store()));
        }
        let dir = std::env::temp_dir().join(format!("viv-sem-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("math.py"), "def add(a, b):\n    return a + b\n").unwrap();
        std::fs::write(dir.join("http.py"), "import urllib.request\n\ndef download(url):\n    return urllib.request.urlopen(url).read()\n").unwrap();
        std::fs::write(dir.join("README.md"), "# Tools\nA few helpers for this project.\n").unwrap();
        let started = std::time::Instant::now();
        let (hits, status) = search(&dir, "function that sums two numbers", 3).await;
        println!("embedder={:?} err={:?} took={:?}", status.embedder.map(|e| e.label()), status.embed_error, started.elapsed());
        for h in &hits {
            println!("{:.2} {}:{}", h.score, h.path, h.start);
        }
        let (hits2, _) = search(&dir, "fetch a web page over the network", 3).await;
        println!("-- second query");
        for h in &hits2 {
            println!("{:.2} {}:{}", h.score, h.path, h.start);
        }
        assert!(status.embedder.is_some(), "no embedding provider — check the keys: {:?}", status.embed_error);
        assert_eq!(hits[0].path, "math.py");
        assert_eq!(hits2[0].path, "http.py");
        if let Some(p) = index_path(&dir) {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
