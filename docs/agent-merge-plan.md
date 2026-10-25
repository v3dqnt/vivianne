# Merge `coding_agent/` into `agent/`: one base agent, many profiles

**When:** after the Oct 30 hackathon submission (user's call). Nothing changes on `hackathon-nvidia` before then.

## Context

The user feels they maintain "two architectures with conflicting jobs" (`src-tauri/src/agent/` and `src-tauri/src/coding_agent/`) and proposes one base agent (all tools, the intelligence), specialised only by prompts and guardrails.

What the code actually shows:

- **There is already one agent loop.** `agent/runtime/loop_core.rs::run_agent_loop_with_prompts` is the only loop. `coding_agent/` has no loop or state; it is a tool library (file/shell/web tools, documents, GitHub, computer use, DIP) plus helpers (compaction, context shaping, the subagent registry, subagent profiles). Merging the folders alone is cosmetic.
- **The real split is in three places:**
  1. **Tools are defined two ways.** The `Tool` trait registry (`coding_agent/tool_exec.rs::create_coding_tools`) covers tools that need only `args` and `cwd`. A 49-branch `if/else` dispatch chain (`agent/tools/exec.rs`) covers tools that need `state` or `app`. Each tool is then described again in `tools/schemas.rs` (schema), `tools/traits.rs` (effect), `tools/profile.rs` (lean/deferred), `tools/capabilities.rs` (prompt group) and the hand-kept `DISPATCH_CHAIN_TOOLS` test list. Adding one tool means editing up to six places.
  2. **Guardrails are scattered knobs.** `AgentState` carries `subagent_allowed_tools`, `delegation_allowed`, `bot_policy`, `auto_approve`, `plan_mode`, `owned_paths` and `persona_mode`, and `tools/exec.rs` checks each one separately (`compute_restrictions`, the plan-mode check, the ownership check, `bots::decide_call`, bypass). Specialisations are each built differently: `SubagentProfile`, `team::to_profile`, `BotPolicy`, `PersonaMode`.
  3. **Four drivers build an agent differently.** These are chat (`runtime/agent.rs`), subagents and team members (`tools/exec.rs::drive_subagent`), bots (`team/bots.rs`, with its own thread, trimming and approvals), and the notch (`instant/session.rs`). Memory `agents-refactor` already records bots as "a second copy of the chat runtime" and an approved post-Oct-30 refactor.

**Recommendation: merge, and the user's model is the right target.** One base agent (loop + every tool), with specialisation as data: an `AgentProfile` that sets prompt, model, tool policy, approvals, scope and delegation. Do it in phases, because the folder move is the cheap part; the value is in phases 2 to 4.

## Phase 0: Preconditions

- Hackathon work is committed and pushed, and the submission is tagged (for example `hackathon-submission`), so every phase can be diffed and reverted.
- No other Claude session is editing `src-tauri/src` (one was reorganising `agent/` on Oct 24). Coordinate first.
- Baseline: full `cargo test --lib` green, and `npx tsc -p tsconfig.app.json --noEmit` in `gui-next` green.

## Phase 1: Folder merge (mechanical, about one session)

Same method as the Oct 24 `agent/memory/` and `agent/tools/` moves: `git mv`, then rewrite paths with `sed`, then fix `super::`, `pub(super)` and `include_str!` relative paths (both bit us on Oct 24).

| `coding_agent/` | New home |
|---|---|
| `tool_exec.rs` (read, write, edit, bash, grep, web...) | `agent/tools/builtin.rs` |
| `apply_patch`, `documents`, `doc_design` | `agent/tools/files/` |
| `github`, `repo_tools`, `semantic_index`, `checkers`, `sandbox` | `agent/tools/code/` |
| `computer`, `dip`, `dip_mac`, `win`, `mac`, `ocr` | `agent/tools/desktop/` (keep the `cfg(target_os)` gates) |
| `custom_tools`, `skills`, `tool_audit`, `commands` | `agent/tools/` |
| `compaction`, `context_shaping`, `streaming` | `agent/runtime/` |
| `orchestration` (subagent registry) | `agent/runtime/subagents.rs` |
| `subagent_profiles` | `agent/profiles/` (seed for phase 3) |

- Rewrite `crate::coding_agent::X` everywhere. This includes 12 files outside `agent/`: `lib.rs`, `mods/tools.rs`, `multiplayer/{collab,state}.rs`, `instant/assist.rs`, `ui/{notch,activity}.rs`, `browser/helium.rs`, `cli_tools/terminal.rs`, `accounts/vault.rs`, `models/nebius.rs`, `system/mac_permissions.rs`.
- Delete `coding_agent/mod.rs`. Keep the `win_cancel` and `win_cancel_reset` helpers in `agent/tools/desktop/mod.rs`.
- **No behaviour change.** This is its own commit.

## Phase 2: One tool definition

One trait in `agent/tools/mod.rs`, replacing both today's `Tool` trait and the dispatch chain:

```rust
#[async_trait(?Send)]          // the loop is already non-Send (see drive_subagent's comment)
pub trait Tool {
    fn name(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;   // next to its impl: one copy, can't drift
    fn effect(&self) -> Effect;              // from tools/traits.rs (read-only, writes, external...)
    async fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> AgentToolResult;
}
pub struct ToolCtx<'a> { cwd: &'a str, state: &'a AgentState, app: &'a AppHandle, emit: &'a Emit, depth: usize, call_id: &'a str }
```

- **Registry:** built-ins are listed once, and dynamic sources are appended per turn (mods via `mods/tools.rs`, MCP, custom tools, playbooks), as `create_coding_tools` does today.
- **Migration without a big bang:** dispatch tries the registry first, then falls back to the old chain. Move one group at a time (file tools, then team, browser, terminal, desktop), running `cargo test --lib` after each. Delete the chain at the end.
- **Becomes derived:**
  - The `tools/schemas.rs` static list becomes `registry.schemas()`.
  - The `tools/traits.rs` table becomes `tool.effect()`.
  - The `DISPATCH_CHAIN_TOOLS` hand list and its consistency tests become one test: every registered tool has a schema and an effect.
- **Keep:** `tools/profile.rs` (lean and deferred loading) and `tools/capabilities.rs` (prompt grouping), both now reading tool names from the registry.

## Phase 3: One profile, one gate (the user's idea)

`agent/profiles/mod.rs`:

```rust
pub struct AgentProfile {
    pub role: Role,                  // name, system-prompt framing, standing instructions, inject AGENTS.md
    pub model: ModelChoice,          // override | route by role/difficulty | inherit
    pub tools: ToolPolicy,           // allow-list | deny | read-only | plan-mode
    pub approvals: ApprovalPolicy,   // bypass | ask-for categories | refuse (generalises BotRules)
    pub scope: Scope,                // workspace folders, access mode, owned paths
    pub delegation: Delegation,      // may spawn subagents? max depth
}
impl AgentProfile {
    pub fn decide(&self, tool: &dyn Tool, args: &Value, cwd: &str) -> Decision; // Run | Ask(why) | Refuse(why)
}
```

- **Presets replace today's ad-hoc builders:**
  - `chat()` is the base agent.
  - `persona(PersonaMode)` covers Ask, Debug and Explore.
  - `plan()` is plan mode.
  - `subagent(&SubagentProfile)` covers the built-in and saved subagent profiles.
  - `employee(&TeamAgent)` replaces `team::to_profile`.
  - `bot(&Bot, Origin)` replaces `BotPolicy`.
  - `instant()` is the notch.
- **State:** `AgentState` holds one `Arc<AgentProfile>` in place of the seven knobs listed under Context.
- **Gate:** in `tools/exec.rs`, a single `profile.decide(...)` call replaces `compute_restrictions`, the persona, plan-mode and ownership checks, `bots::decide_call` and the bypass logic. Two global layers stay outside the profile: tools the user disabled in Settings, and the lean-provider tool list.
- **Characterisation first (security-relevant):** before touching the gate, write a table test of every preset against representative tools, recording today's decision (Run, Ask or Refuse). The refactor must keep that table identical. This includes:
  - plan mode refuses `write`;
  - Ask mode refuses `edit`;
  - an employee's `apply_patch` outside its owned paths is refused;
  - a proactive bot only reads the web;
  - `vault fill` and `nebius_job submit` always ask.

## Phase 4: One driver (the approved `agents-refactor`)

- `agent/runtime/spawn.rs`: `spawn_agent(profile, input, thread) -> RunHandle` builds the `AgentState`, the system prompt (`runtime/directive.rs`) and the run record in one way.
- **Callers:**
  - chat (`runtime/agent.rs`);
  - `tools/exec.rs::drive_subagent`, which then deletes its own role framing and model routing;
  - `team/run.rs` members;
  - `team/bots.rs`;
  - `instant/session.rs`.
- **Bots become profile plus a real `AgentRegistry` thread, per memory `agents-refactor`.**
  - On the frontend, delete `BotChat`, `BotSteps`, `LiveRunCard`, the `dm.jsonl` mirror, the custom trim, and the plan and checkpoint shims.
  - The bot's DM is the main ChatView bound to its thread.
- **Prerequisite:** thread-scoped stop, steer, queue, revert and compaction (`docs/session-windows-plan.md`). Do that first; session pop-out windows are blocked on the same thing.

## Critical files

- `src-tauri/src/agent/tools/exec.rs`: the dispatch chain, the gate, `drive_subagent` and `execute_task_tool`.
- `src-tauri/src/coding_agent/tool_exec.rs`: the `Tool` trait and `create_coding_tools`.
- `src-tauri/src/agent/tools/{schemas,traits,profile,capabilities}.rs`
- `src-tauri/src/agent/runtime/{state,loop_core,agent,directive}.rs`
- `src-tauri/src/coding_agent/subagent_profiles.rs`, `agent/team/{mod,run,bots}.rs` (the `to_profile` and `BotPolicy` builders)
- `src-tauri/src/instant/session.rs`, `src-tauri/src/mods/tools.rs`

## Verification

**Every phase:**
- Full `cargo test --lib` (trusted gate per memory `cargo-test-entrypoint-crash`).
- `cargo check --lib` on Windows.
- The macOS CI check, because of the `cfg(target_os)` desktop tools.

**Phase 2:**
- The new "every tool has a schema and an effect" test.
- Snapshot the model-visible tool list (names and schemas) before and after; the two must be identical.

**Phase 3:** the characterisation decision table must be unchanged.

**Phase 4:**
- `npx tsc -p tsconfig.app.json --noEmit` in `gui-next`.

**Manual run in the app** (after phases 3 and 4):
- A chat turn using file tools.
- Plan mode refusing a write.
- Ask persona refusing an edit.
- A background `task` subagent.
- A `team_run` where one member hits an ownership refusal.
- A bot whose "ask first" rule raises an approval.
- A notch voice command.
