//! LSP — a best-effort Language Server Protocol client. Implemented directly
//! against the public LSP spec (JSON-RPC 2.0 over stdio, Content-Length
//! framing — see https://microsoft.github.io/language-server-protocol/),
//! not tied to any particular existing implementation.
//!
//! Auto-detects a language server on PATH for whatever language a touched
//! file belongs to, spawns it lazily, and aggregates `textDocument/publishDiagnostics`
//! notifications so tools (edit/write/apply_patch) can surface fresh errors
//! right after a change.

pub mod client;
pub mod edits;
pub mod ledger;
pub mod manager;
pub mod servers;
pub mod uri;

use std::collections::HashMap;

/// Standard LSP `languageId` values, keyed by file extension (without the dot).
/// This is public protocol metadata (the languageId identifiers themselves are
/// part of the LSP spec / widely-used convention), not derived from any single
/// implementation.
pub fn language_id_for_extension(ext: &str) -> Option<&'static str> {
    let map: &[(&str, &str)] = &[
        ("ts", "typescript"), ("tsx", "typescriptreact"), ("mts", "typescript"), ("cts", "typescript"),
        ("js", "javascript"), ("jsx", "javascriptreact"), ("mjs", "javascript"), ("cjs", "javascript"),
        ("py", "python"), ("pyi", "python"),
        ("rs", "rust"),
        ("go", "go"),
        ("c", "c"), ("h", "c"),
        ("cpp", "cpp"), ("cxx", "cpp"), ("cc", "cpp"), ("hpp", "cpp"), ("hxx", "cpp"),
        ("cs", "csharp"),
        ("java", "java"),
        ("kt", "kotlin"), ("kts", "kotlin"),
        ("rb", "ruby"), ("rake", "ruby"), ("gemspec", "ruby"),
        ("php", "php"),
        ("swift", "swift"),
        ("scala", "scala"),
        ("lua", "lua"),
        ("sh", "shellscript"), ("bash", "shellscript"), ("zsh", "shellscript"),
        ("ps1", "powershell"), ("psm1", "powershell"),
        ("html", "html"), ("htm", "html"),
        ("css", "css"), ("scss", "scss"), ("sass", "sass"), ("less", "less"),
        ("json", "json"), ("jsonc", "jsonc"),
        ("yaml", "yaml"), ("yml", "yaml"),
        ("toml", "toml"),
        ("md", "markdown"), ("markdown", "markdown"),
        ("vue", "vue"),
        ("svelte", "svelte"),
        ("dart", "dart"),
        ("ex", "elixir"), ("exs", "elixir"),
        ("erl", "erlang"), ("hrl", "erlang"),
        ("hs", "haskell"), ("lhs", "haskell"),
        ("clj", "clojure"), ("cljs", "clojure"), ("cljc", "clojure"),
        ("zig", "zig"),
        ("sql", "sql"),
        ("graphql", "graphql"), ("gql", "graphql"),
        ("proto", "proto"),
        ("tf", "terraform"), ("tfvars", "terraform"),
        ("ml", "ocaml"), ("mli", "ocaml"),
        ("nix", "nix"),
    ];
    map.iter().find(|(e, _)| *e == ext).map(|(_, id)| *id)
}

/// Candidate language-server launch commands per `languageId`, tried in order
/// until one resolves on PATH. Multiple candidates cover common alternates
/// (e.g. pyright vs pylsp for Python).
pub fn server_candidates(language_id: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match language_id {
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => &[
            ("typescript-language-server", &["--stdio"]),
        ],
        "python" => &[
            ("pyright-langserver", &["--stdio"]),
            ("basedpyright-langserver", &["--stdio"]),
            ("pylsp", &[]),
            ("jedi-language-server", &[]),
        ],
        "rust" => &[
            ("rust-analyzer", &[]),
        ],
        "go" => &[
            ("gopls", &[]),
        ],
        "c" | "cpp" => &[
            ("clangd", &[]),
        ],
        "csharp" => &[
            ("csharp-ls", &[]),
            ("omnisharp", &["-lsp"]),
        ],
        "ruby" => &[
            ("ruby-lsp", &[]),
            ("solargraph", &["stdio"]),
        ],
        "java" => &[("jdtls", &[])],
        "dart" => &[("dart", &["language-server", "--protocol=lsp"])],
        "svelte" => &[("svelteserver", &["--stdio"])],
        "markdown" => &[("marksman", &["server"])],
        "toml" => &[("taplo", &["lsp", "stdio"])],
        "clojure" => &[("clojure-lsp", &[])],
        "erlang" => &[("erlang_ls", &[])],
        "ocaml" => &[("ocamllsp", &[])],
        "nix" => &[("nil", &[]), ("nixd", &[])],
        "php" => &[
            ("intelephense", &["--stdio"]),
        ],
        "lua" => &[
            ("lua-language-server", &[]),
        ],
        "html" => &[("vscode-html-language-server", &["--stdio"])],
        "css" | "scss" | "sass" | "less" => &[("vscode-css-language-server", &["--stdio"])],
        "json" | "jsonc" => &[("vscode-json-language-server", &["--stdio"])],
        "yaml" => &[("yaml-language-server", &["--stdio"])],
        "bash" | "shellscript" => &[("bash-language-server", &["start"])],
        "dockerfile" => &[("docker-langserver", &["--stdio"])],
        "terraform" => &[("terraform-ls", &["serve"])],
        "graphql" => &[("graphql-lsp", &["server", "-m", "stream"])],
        "vue" => &[("vue-language-server", &["--stdio"])],
        "haskell" => &[("haskell-language-server-wrapper", &["--lsp"])],
        "elixir" => &[("elixir-ls", &[])],
        "zig" => &[("zls", &[])],
        "swift" => &[("sourcekit-lsp", &[])],
        "kotlin" => &[("kotlin-language-server", &[])],
        "scala" => &[("metals", &[])],
        _ => &[],
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<u8>,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
}

/// Format one file's diagnostics as a small `<diagnostics>` block for tool output,
/// capped (`ledger::MAX_LISTED`) so large error counts don't bloat context.
pub fn format_diagnostics(file: &str, diagnostics: &[Diagnostic]) -> Option<String> {
    format_with_note(file, diagnostics, None)
}

/// The same, with a closing line of its own (for example how many earlier errors are unchanged).
pub fn format_with_note(file: &str, diagnostics: &[Diagnostic], note: Option<&str>) -> Option<String> {
    let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity.unwrap_or(1) == 1).collect();
    if errors.is_empty() {
        return None;
    }
    let severity_label = |s: u8| match s {
        1 => "ERROR",
        2 => "WARN",
        3 => "INFO",
        _ => "HINT",
    };
    let lines: Vec<String> = errors
        .iter()
        .take(ledger::MAX_LISTED)
        .map(|d| format!("{} [{}:{}] {}", severity_label(d.severity.unwrap_or(1)), d.range.start.line + 1, d.range.start.character + 1, d.message))
        .collect();
    let more = errors.len().saturating_sub(ledger::MAX_LISTED);
    let mut body = lines.join("\n");
    if more > 0 {
        body.push_str(&format!("\n... and {more} more"));
    }
    if let Some(n) = note {
        body.push('\n');
        body.push_str(n);
    }
    Some(format!("<diagnostics file=\"{file}\">\n{body}\n</diagnostics>"))
}

pub type DiagnosticsByFile = HashMap<String, Vec<Diagnostic>>;
