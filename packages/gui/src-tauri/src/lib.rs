use std::sync::Arc;

use tauri::Manager;

pub mod ai;
use crate::ai::auth::{AuthState, auth_read_credential, auth_store_api_key, auth_store_oauth, auth_delete_credential, auth_is_authenticated, auth_resolve_api_key, CredentialPool, cmd_pool_list, cmd_pool_add, cmd_pool_remove, cmd_pool_set_strategy, cmd_pool_get_strategy, cmd_pool_reset_cooldowns};
pub mod agent;
use crate::agent::commands::AppState;
pub mod accounts;
use crate::accounts::AccountState;
pub mod multiplayer;
use crate::multiplayer::MultiplayerState;

// New modules
pub mod coding_agent;
pub mod lsp;
pub mod mcp;
mod file_preview;

/// Makes `cargo test` able to run at all on Windows.
///
/// This crate imports `TaskDialogIndirect`, `SetWindowSubclass`,
/// `RemoveWindowSubclass` and `DefSubclassProc` (through wry/tauri). All four
/// exist only in **version 6** of `comctl32.dll`, which Windows hands out only
/// to a binary whose manifest declares a dependency on
/// `Microsoft.Windows.Common-Controls` 6.0.0.0. Anything else is bound to the
/// v5 `comctl32` in System32, which does not export them — so the process dies
/// during module load with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) before
/// `main` is ever reached.
///
/// `tauri-build` embeds that manifest into `app.exe`, which is why the app runs
/// fine. It does not embed one into the unit-test harness binary Cargo builds
/// from this same crate, which is why `cargo test` failed on this repo for its
/// entire history with an error that looked like a toolchain fault and had
/// nothing to do with any test.
///
/// Emitting the linker directive from the crate itself fixes every binary built
/// from it — app, tests, examples, benches — rather than just the one
/// `tauri-build` knows about. `.drectve` is the COFF section the linker reads
/// command-line options from; `#[used]` keeps it from being stripped.
#[cfg(windows)]
#[used]
#[link_section = ".drectve"]
static COMMON_CONTROLS_V6_MANIFEST: [u8; 167] = *br#" /MANIFESTDEPENDENCY:"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'""#;

/// Set when Viv Instant's global wake chord can't be registered at startup —
/// most often because another running application already claims it. Recorded
/// rather than only logged: the chord is the ONLY way into Viv Instant, so a
/// clash makes the whole feature unreachable and looks broken rather than
/// blocked. Settings reads this back via `cmd_instant_shortcut_error`.
static INSTANT_SHORTCUT_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The human-readable wake chord, matching what `setup` actually registers.
///
/// macOS reserves Ctrl+Shift+<key> chords far more aggressively than Windows
/// does, and Control is not the modifier a Mac user reaches for; the platform
/// convention for an app-level chord is Command. The label lives in Rust
/// rather than being re-derived in the UI so the string the user is told to
/// press and the chord actually registered can never drift apart.
pub const INSTANT_SHORTCUT_LABEL: &str =
  if cfg!(target_os = "macos") { "\u{2318}\u{21e7}Space" } else { "Ctrl+Shift+Space" };

#[tauri::command]
fn cmd_instant_shortcut_error() -> Option<String> {
  INSTANT_SHORTCUT_ERROR.lock().ok().and_then(|s| s.clone())
}

#[tauri::command]
fn cmd_instant_shortcut_label() -> &'static str {
  INSTANT_SHORTCUT_LABEL
}
mod updates;
pub mod camoufox;
pub mod browser_sessions;
pub mod live_browser;
pub mod live_browser_agent;
pub mod docker_installer;
pub mod optional_components;
pub mod background;
pub mod instant;
pub mod overlays;
pub mod activity;
pub mod helium;
pub mod automation;
pub mod terminal;
use crate::automation::AutomationStore;
use crate::agent::llama_manager::LlamaManager;
use crate::agent::dev_server_manager::DevServerManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // A profile (VIV_PROFILE) is a separate Vivianne on the same machine; its
  // webview storage (layout, last thread, …) must be separate too, or two
  // instances would share and overwrite each other's UI state.
  #[cfg(windows)]
  if crate::agent::paths::profile().is_some() {
    if let Ok(home) = crate::agent::paths::viv_home() {
      let dir = home.join("webview");
      let _ = std::fs::create_dir_all(&dir);
      std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &dir);
    }
  }
  tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_store::Builder::default().build())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(
      tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
          // The wake chord is push-to-talk: pressed starts listening,
          // released sends what was said. Escape acts on press only.
          let pressed = event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed;
          if shortcut.key != tauri_plugin_global_shortcut::Code::Escape {
            if pressed {
              crate::instant::window::press(app);
            } else {
              crate::instant::window::release(app);
            }
            return;
          }
          if !pressed {
            return;
          }
          // Two chords share this one handler, told apart by key. Escape is
          // registered only for the duration of a run (see
          // `instant::window::arm_escape`) — it has to be a GLOBAL shortcut
          // rather than a keydown listener in the overlay, because the overlay
          // deliberately never takes focus: if it did, the keystrokes Viv
          // Instant is typing would land in the overlay instead of the app it
          // is driving.
          crate::instant::window::abort_from_shortcut(app);
        })
        .build(),
    )
    .invoke_handler(tauri::generate_handler![
      // Auth commands
      auth_read_credential,
      auth_store_api_key,
      auth_store_oauth,
      auth_delete_credential,
      auth_is_authenticated,
      auth_resolve_api_key,
      crate::ai::auth::cmd_list_available_models,
      // Credential pool commands
      cmd_pool_list,
      cmd_pool_add,
      cmd_pool_remove,
      cmd_pool_set_strategy,
      cmd_pool_get_strategy,
      cmd_pool_reset_cooldowns,
      // Agent commands
      crate::agent::commands::cmd_agent_prompt,
      crate::agent::commands::cmd_agent_continue,
      crate::agent::commands::cmd_agent_reset,
      crate::agent::commands::cmd_agent_abort,
      crate::agent::commands::cmd_agent_steer,
      crate::agent::commands::cmd_agent_steering_queue,
      crate::agent::commands::cmd_agent_follow_up_queue,
      crate::agent::commands::cmd_agent_clear_queues,
      crate::agent::commands::cmd_agent_get_queues,
      crate::agent::commands::cmd_agent_remove_queued,
      crate::agent::commands::cmd_agent_messages,
      crate::agent::commands::cmd_agent_set_model,
      crate::agent::commands::cmd_agent_set_thinking_level,
      crate::agent::commands::cmd_agent_set_persona_mode,
      crate::agent::commands::cmd_agent_set_compaction_config,
      crate::agent::commands::cmd_agent_get_compaction_config,
      crate::agent::commands::cmd_compact_now,
      crate::agent::commands::cmd_agent_get_workspace,
      crate::agent::commands::cmd_agent_set_workspace,
      crate::agent::commands::cmd_create_workspace_folder,
      crate::agent::commands::cmd_agent_set_steering_mode,
      crate::agent::commands::cmd_agent_set_follow_up_mode,
      crate::agent::commands::cmd_debug_agent_status,
      crate::agent::commands::cmd_agent_session_stats,
      crate::agent::commands::cmd_get_models_config,
      crate::agent::commands::cmd_save_models_config,
      crate::agent::commands::cmd_get_settings,
      crate::agent::days::cmd_open_today,
      crate::agent::days::cmd_list_days,
      crate::agent::days::cmd_read_session_messages,
      crate::agent::days::cmd_list_archive,
      crate::agent::master::cmd_draft_charter,
      crate::agent::master::cmd_create_master,
      crate::agent::master::cmd_get_master,
      crate::agent::master::cmd_master_kickoff_prompt,
      crate::agent::master::cmd_onboarding_questions,
      crate::agent::master::cmd_open_agent_file,
      crate::agent::team::cmd_list_team,
      crate::agent::team::cmd_save_team_agent,
      crate::agent::team::cmd_approve_team_agent,
      crate::agent::team::cmd_delete_team_agent,
      crate::agent::conversation::cmd_open_conversation,
      crate::agent::conversation::cmd_read_conversation_archive,
      crate::agent::runs::cmd_list_agent_runs,
      crate::agent::runs::cmd_read_run_messages,
      crate::agent::runs::cmd_comment_on_run,
      crate::agent::office::cmd_read_document_bytes,
      crate::agent::commands::cmd_system_roots,
      crate::multiplayer::commands::cmd_multiplayer_launch_test_partner,
      crate::multiplayer::commands::cmd_multiplayer_test_partner_id,
      crate::multiplayer::commands::cmd_app_profile,
      crate::multiplayer::collab::cmd_multiplayer_collab,
      crate::multiplayer::collab::cmd_multiplayer_dismiss_notes,
      crate::agent::office::cmd_edit_document_part,
      crate::agent::office::cmd_restore_document_bytes,
      crate::agent::review::cmd_assignment_changes,
      crate::agent::review::cmd_undo_assignment,
      crate::agent::commands::cmd_list_tool_names,
      crate::agent::commands::cmd_register_local_endpoint,
      crate::agent::commands::cmd_unregister_local_endpoint,
      crate::agent::commands::cmd_list_local_endpoints,
      crate::agent::llama_manager::cmd_llama_start,
      crate::agent::llama_manager::cmd_llama_stop,
      crate::agent::llama_manager::cmd_llama_status,
      crate::agent::llama_manager::cmd_llama_list,
      crate::agent::llama_manager::cmd_llama_register,
      crate::agent::llama_manager::cmd_llama_unregister,
      crate::agent::dev_server_manager::cmd_dev_server_register,
      crate::agent::dev_server_manager::cmd_dev_server_unregister,
      crate::agent::dev_server_manager::cmd_dev_server_start,
      crate::agent::dev_server_manager::cmd_dev_server_stop,
      crate::agent::dev_server_manager::cmd_dev_server_list,
      crate::agent::commands::cmd_save_settings,
      crate::agent::commands::cmd_get_sessions,
      crate::agent::commands::cmd_get_active_session_id,
      crate::agent::projects::cmd_list_projects,
      crate::agent::projects::cmd_create_project,
      crate::agent::projects::cmd_create_multiplayer_project,
      crate::agent::projects::cmd_update_project,
      crate::agent::projects::cmd_open_external,
      crate::agent::projects::cmd_delete_project,
      crate::agent::projects::cmd_get_session_project_map,
      crate::agent::projects::cmd_assign_session_project,
      crate::agent::commands::cmd_delete_session,
      crate::agent::commands::cmd_write_text_file,
      crate::agent::commands::cmd_read_file_raw,
      crate::agent::commands::cmd_load_session,
      crate::agent::commands::cmd_save_current_session,
      crate::agent::commands::cmd_rename_session,
      crate::agent::commands::cmd_new_session,
      crate::agent::commands::cmd_agent_revert_to_index,
      crate::agent::commands::cmd_get_usage_history,
      crate::agent::commands::cmd_telemetry_track,
      crate::agent::commands::cmd_get_telemetry_summary,
      crate::agent::commands::cmd_list_checkpoints,
      crate::agent::commands::cmd_inspect_document,
      crate::browser_sessions::cmd_browser_session_status,
      crate::browser_sessions::cmd_browser_import_cookies,
      crate::browser_sessions::cmd_browser_reset_session,
      crate::browser_sessions::cmd_browser_cookies_dir,
      crate::browser_sessions::cmd_browser_list_cookie_files,
      crate::agent::commands::cmd_restore_checkpoint,
      crate::agent::commands::cmd_clear_telemetry,
      crate::agent::commands::cmd_submit_bug_report,
      // Updates / changelog / announcements
      crate::file_preview::cmd_read_file_preview,
      cmd_instant_shortcut_error,
      cmd_instant_shortcut_label,
      crate::background::cmd_hide_to_background,
      crate::background::cmd_quit_app,
      crate::updates::cmd_app_version,
      crate::updates::cmd_get_changelog,
      crate::updates::cmd_get_update_state,
      crate::updates::cmd_mark_version_seen,
      crate::updates::cmd_dismiss_announcement,
      crate::updates::cmd_get_announcements,
      crate::updates::cmd_check_for_update,
      crate::updates::cmd_install_update,
      crate::agent::commands::cmd_session_cost,
      crate::agent::commands::cmd_list_dir_tree,
      crate::agent::commands::cmd_read_file_content,
      crate::agent::notifications::cmd_test_notification,
      crate::agent::commands::cmd_fork_session,
      crate::agent::commands::cmd_clone_session,
      crate::agent::commands::cmd_list_skills,
      crate::agent::commands::cmd_save_skill_file,
      crate::agent::commands::cmd_list_playbooks,
      crate::coding_agent::custom_tools::cmd_list_custom_tools,
      crate::coding_agent::custom_tools::cmd_delete_custom_tool,
      crate::coding_agent::custom_tools::cmd_custom_tool_source,
      crate::agent::commands::cmd_add_playbook_file,
      crate::agent::commands::cmd_save_playbook,
      crate::agent::commands::cmd_remove_playbook,
      crate::agent::commands::cmd_list_subagent_profiles,
      crate::agent::commands::cmd_save_subagent_profile,
      crate::agent::commands::cmd_delete_subagent_profile,
      crate::agent::commands::cmd_agent_respond_permission,
      crate::agent::commands::cmd_agent_answer_question,
      crate::agent::commands::cmd_agent_respond_confirmation,
      crate::agent::commands::cmd_memory_status,
      crate::agent::commands::cmd_memory_enable,
      crate::agent::commands::cmd_memory_disable,
      crate::agent::commands::cmd_memory_configure,
      crate::agent::commands::cmd_memory_configure_extraction,
      crate::agent::commands::cmd_memory_purge,
      crate::agent::commands::cmd_memory_remember,
      crate::agent::commands::cmd_memory_forget,
      crate::agent::commands::cmd_memory_recall,
      crate::agent::commands::cmd_mcp_add_server,
      crate::agent::commands::cmd_mcp_remove_server,
      crate::agent::commands::cmd_mcp_list_servers,
      crate::agent::commands::cmd_mcp_list_tools,
      crate::agent::commands::cmd_mcp_authorize,
      crate::agent::commands::cmd_web_preview_navigate,
      crate::agent::commands::cmd_web_preview_back,
      crate::agent::commands::cmd_web_preview_forward,
      crate::agent::commands::cmd_web_preview_refresh,
      crate::agent::commands::cmd_web_preview_screenshot,
      crate::agent::commands::cmd_web_preview_click_at,
      crate::agent::commands::cmd_web_preview_type,
      crate::agent::commands::cmd_web_preview_press,
      crate::agent::commands::cmd_web_preview_scroll,
      crate::live_browser::cmd_live_browser_show,
      crate::live_browser::cmd_live_browser_hide,
      crate::live_browser::cmd_live_browser_close,
      crate::live_browser::cmd_live_browser_url,
      crate::live_browser::cmd_live_browser_eval,
      crate::agent::commands::cmd_check_docker_status,
      crate::agent::commands::cmd_install_docker,
      crate::optional_components::cmd_get_optional_components_status,
      crate::optional_components::cmd_start_optional_components_download,
      crate::agent::commands::cmd_terminal_list_shells,
      crate::agent::commands::cmd_terminal_spawn,
      crate::agent::commands::cmd_terminal_write,
      crate::agent::commands::cmd_terminal_resize,
      crate::agent::commands::cmd_terminal_close,
      crate::agent::commands::cmd_lsp_diagnostics,
      crate::agent::commands::cmd_git_current_branch,
      crate::agent::commands::cmd_terminal_list_sessions,
      crate::agent::commands::cmd_git_remote,
      crate::agent::commands::cmd_git_log,
      crate::agent::commands::cmd_git_branches,
      crate::agent::commands::cmd_git_status,
      crate::agent::commands::cmd_git_diff_stat,
      crate::agent::commands::cmd_git_diff_file,
      crate::agent::commands::cmd_git_checkout_branch,
      crate::agent::commands::cmd_git_create_branch,
      crate::agent::commands::cmd_get_project_dir,
      crate::agent::commands::cmd_set_project_dir,
      // Grok-parity coding-agent commands (subagents, plan mode, ask-user)
      crate::coding_agent::commands::cmd_coding_list_subagents,
      crate::coding_agent::commands::cmd_coding_get_subagent_messages,
      crate::coding_agent::commands::cmd_coding_kill_subagent,
      crate::agent::plans::cmd_plan_list,
      crate::agent::plans::cmd_plan_get,
      crate::agent::plans::cmd_plan_mode_get,
      crate::agent::plans::cmd_plan_mode_set,
      crate::agent::plans::cmd_plan_approve,
      crate::agent::plans::cmd_plan_cancel,
      crate::agent::team_run::cmd_team_run_get,
      crate::agent::run_ledger::cmd_long_task_get,
      crate::agent::run_ledger::cmd_long_task_control,
      crate::agent::run_ledger::cmd_long_task_resume_prompt,
      crate::coding_agent::commands::cmd_coding_register_stream_channel,
      crate::coding_agent::computer::cmd_capture_screen,
      crate::instant::stt::cmd_instant_stt_preload,
      crate::instant::stt::cmd_instant_stt_transcribe,
      crate::instant::session::cmd_instant_run,
      crate::instant::session::cmd_instant_abort,
      crate::instant::session::cmd_instant_steps,
      crate::instant::session::cmd_instant_cancel_step,
      crate::instant::window::cmd_instant_set_size,
      crate::instant::window::cmd_instant_dismiss,
      crate::instant::pointer::cmd_pointer_point,
      crate::instant::pointer::cmd_pointer_highlight,
      crate::instant::pointer::cmd_pointer_hide,
      crate::overlays::cmd_overlay_ready,
      crate::instant::stt::cmd_instant_stt_installed,
      crate::instant::stt::cmd_parakeet_status,
      crate::activity::cmd_activity_set_size,
      crate::ai::chatgpt::cmd_chatgpt_status,
      crate::helium::cmd_helium_status,
      crate::helium::cmd_helium_open_extension_folder,
      crate::agent::bots::cmd_bots_room,
      crate::agent::bots::cmd_bots_post,
      crate::agent::bots::cmd_bots_dm,
      crate::agent::bots::cmd_bots_dm_send,
      crate::agent::bots::cmd_bots_status,
      crate::agent::bots::cmd_bots_settings,
      crate::agent::bots::cmd_bots_save_settings,
      crate::agent::bots::cmd_bots_stop,
      crate::agent::bots::cmd_bots_reset,
      crate::agent::bots::cmd_bots_memory,
      crate::agent::bots::cmd_bots_save_memory,
      crate::agent::bots::cmd_bots_routines,
      crate::agent::bots::cmd_bots_save_routine,
      crate::agent::bots::cmd_bots_delete_routine,
      crate::agent::bots::cmd_bots_run_routine,
      crate::ai::chatgpt::cmd_chatgpt_sign_in,
      crate::ai::chatgpt::cmd_chatgpt_sign_out,
      crate::ai::chatgpt::cmd_chatgpt_manage_usage,
      crate::agent::commands::cmd_activity_stop,
      crate::instant::stt::cmd_parakeet_download,
      crate::instant::stt::cmd_parakeet_remove,
      // Automation (scheduled agent tasks)
      crate::automation::cmd_automation_list,
      crate::automation::cmd_automation_create,
      crate::automation::cmd_automation_update,
      crate::automation::cmd_automation_set_enabled,
      crate::automation::cmd_automation_delete,
      crate::automation::cmd_automation_run_now,
      // Account commands
      crate::accounts::commands::cmd_account_sign_in_with_browser,
      crate::accounts::commands::cmd_account_sign_out,
      crate::accounts::commands::cmd_account_get_session,
      crate::accounts::commands::cmd_account_update_profile,
      // Multiplayer commands
      crate::multiplayer::commands::cmd_multiplayer_my_id,
      crate::multiplayer::commands::cmd_multiplayer_status,
      crate::multiplayer::commands::cmd_multiplayer_connect,
      crate::multiplayer::commands::cmd_multiplayer_disconnect,
      crate::multiplayer::commands::cmd_multiplayer_get_display_name,
      crate::multiplayer::commands::cmd_multiplayer_set_display_name,
      crate::multiplayer::commands::cmd_multiplayer_peer_messages,
      crate::multiplayer::commands::cmd_multiplayer_chat_log,
      crate::multiplayer::commands::cmd_multiplayer_send_chat,
    ])
    .setup(|app| {
      // Installed first thing in setup so a panic anywhere in the rest of
      // startup is still recorded. Chains to the previous hook, so this only
      // adds a crash file — it never suppresses the normal panic output.
      crate::agent::telemetry::install_panic_hook(app.package_info().version.to_string());

      // Tray icon + close-to-background. Installed early so the close handler
      // is attached before the user can possibly reach the close button, and
      // so Viv Instant's chord keeps working once the window is dismissed.
      crate::background::init(app.handle());
      crate::agent::telemetry::track("app.start", serde_json::json!({
        "version": app.package_info().version.to_string(),
        "os": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
      }));

      // Abandoned "New chat" threads are persisted on creation, so clear out
      // any that were never used. Safe here specifically: nothing is active
      // at startup — that invariant only requires running before anything
      // else touches the session dir, not before first paint, so this runs
      // on its own thread rather than blocking `.setup()` (and the window
      // show that waits on it). Spilled tool output (see
      // `coding_agent::context_shaping`) has no owning session to get
      // cleaned up with, so it ages out on a timer here too, same reasoning.
      std::thread::spawn(|| {
        let pruned = crate::agent::commands::prune_empty_sessions();
        if pruned > 0 {
          log::info!("[sessions] pruned {pruned} empty session(s)");
        }
        let pruned_spill = crate::coding_agent::context_shaping::prune_old_spill_files();
        if pruned_spill > 0 {
          log::info!("[spill] pruned {pruned_spill} old spill file(s)");
        }
      });

      // Initialize auth state + credential pool together, in one blocking
      // setup call instead of two sequential ones — both need the same
      // "is the OS keychain reachable" probe (a real blocking syscall, not
      // free), so it's computed once and shared, and their independent I/O
      // (migration check, pool runtime-state file load) runs concurrently
      // via `tokio::join!` rather than one after the other.
      let (auth_state, credential_pool) = tauri::async_runtime::block_on(async {
        let keychain_available = crate::ai::keychain_store::KeychainCredentialStore::is_available();
        let app_data_dir = app.path().app_data_dir().ok();

        let auth_state_fut = async {
          if keychain_available {
            // One-time migration from a legacy plaintext credentials.json, if
            // present — run against a standalone store instance before
            // building the one `AuthState` actually uses; both talk to the
            // same OS keychain, so there's no shared-state concern here.
            if let Some(dir) = &app_data_dir {
              let legacy_path = dir.join("credentials.json");
              let migration_store = crate::ai::keychain_store::KeychainCredentialStore::new();
              match crate::ai::keychain_store::migrate_plaintext_credentials(&legacy_path, &migration_store).await {
                Ok(n) if n > 0 => log::info!("Migrated {n} credential(s) from plaintext credentials.json into the OS keychain."),
                Ok(_) => {}
                Err(e) => log::warn!("Credential migration to keychain failed (plaintext file left in place): {e}"),
              }
            }
            Arc::new(AuthState::with_keychain_store())
          } else {
            log::warn!(
              "OS keychain is not available on this system — falling back to credentials.json on disk, which is NOT encrypted. \
               API keys will be stored in plaintext until this environment has a working credential backend."
            );
            if let Some(dir) = &app_data_dir {
              let creds_path = dir.join("credentials.json");
              match AuthState::with_file_store(creds_path).await {
                Ok(state) => Arc::new(state),
                Err(e) => {
                  log::warn!("Failed to load persistent auth store: {}", e);
                  Arc::new(AuthState::new())
                }
              }
            } else {
              Arc::new(AuthState::new())
            }
          }
        };

        // Credential pool — same keychain-vs-file backend choice as
        // `AuthState` above (its secrets deserve the same protection), but
        // runtime state (cooldowns/counters) is never sensitive so it always
        // goes to a plain JSON file regardless of which backend won.
        let credential_pool_fut = async {
          let runtime_path = app_data_dir
            .as_ref()
            .map(|d| d.join("credential_pool_state.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("credential_pool_state.json"));
          let runtime = Arc::new(crate::ai::auth::PoolRuntimeStore::new(runtime_path).await);

          let secrets: Arc<dyn crate::ai::auth::PoolSecretsStore> = if keychain_available {
            Arc::new(crate::ai::auth::KeychainPoolSecrets::new())
          } else {
            let path = app_data_dir
              .as_ref()
              .map(|d| d.join("credential_pool_secrets.json"))
              .unwrap_or_else(|| std::path::PathBuf::from("credential_pool_secrets.json"));
            match crate::ai::auth::FilePoolSecrets::new(path).await {
              Ok(store) => Arc::new(store),
              Err(e) => {
                log::warn!("Failed to load credential pool secrets store ({e}) — extra pool keys won't persist across restarts this session.");
                Arc::new(crate::ai::auth::InMemoryPoolSecrets::new())
              }
            }
          };

          Arc::new(CredentialPool { secrets, runtime })
        };

        tokio::join!(auth_state_fut, credential_pool_fut)
      });
      // Also reachable without an AppHandle, for tools that run outside a
      // command (the web search tool reads a search API key from here).
      crate::ai::auth::set_global(auth_state.clone());
      app.manage(auth_state);
      app.manage(credential_pool);

      // Initialize app state
      let app_state = Arc::new(AppState::default());
      app.manage(app_state.clone());
      // Registry for concurrent chat-thread panes — the primary/default
      // thread routes to the same `app_state` above (zero change for every
      // existing command that isn't thread-aware); secondary panes opened
      // via drag-to-dock get their own independent entry.
      app.manage(crate::agent::commands::AgentRegistry::new(app_state));

      // Account system: load whatever session the OS keychain has (if any)
      // before the window shows, so a restart doesn't force a re-login —
      // same "resolve before managing" shape as the auth_state/credential_pool
      // block above.
      let account_state = Arc::new(AccountState::default());
      tauri::async_runtime::block_on(crate::accounts::load_persisted_session(&account_state));
      app.manage(account_state);

      // Multiplayer: bind the P2P endpoint and start listening for incoming
      // pairing connections. A bind failure (e.g. no usable network
      // interface) degrades to multiplayer commands erroring clearly
      // instead of crashing app startup over an optional feature.
      match tauri::async_runtime::block_on(MultiplayerState::new()) {
        Ok(mp_state) => {
          let mp_state = Arc::new(mp_state);
          app.manage(mp_state.clone());
          let mp_app_handle = app.handle().clone();
          tauri::async_runtime::spawn(async move {
            mp_state.run_accept_loop(mp_app_handle).await;
          });
        }
        Err(e) => log::warn!("[multiplayer] failed to start: {e}"),
      }

      // Initialize local endpoint registry
      app.manage(crate::ai::local_registry::new_registry());

      // Initialize llama-server process manager
      app.manage(Arc::new(LlamaManager::new()));

      // Local OCR's tool dispatcher has no AppHandle of its own — this is how
      // it reaches the managed LlamaManager to pause local models for RAM.
      crate::coding_agent::ocr::install_app_handle(app.handle().clone());

      // Initialize dev-server process manager (Live Preview panel)
      app.manage(Arc::new(DevServerManager::default()));

// Grok-parity: streaming_bash / streaming_powershell push live output
      // over a Tauri `Channel` (registered by the frontend via
      // `cmd_coding_register_stream_channel`), not a global `emit()` — see
      // that command's doc comment for why.

      // Automation store + background scheduler (emits `automation_fired`).
      app.manage(Arc::new(AutomationStore::new()));
      crate::automation::spawn_scheduler(app.handle().clone());
      crate::agent::bots::spawn_scheduler(app.handle().clone());
      crate::helium::start(app.handle().clone());

      // PTY-backed terminal sessions (one per open tab).
      app.manage(Arc::new(crate::terminal::TerminalRegistry::new()));
      app.manage(Arc::new(crate::instant::session::InstantSession::default()));

      // Reconnect every MCP server persisted from the previous session.
      // Spawned rather than blocking startup on `block_on` — a slow or
      // unreachable server (a stdio command that never initializes, a
      // remote HTTP server that's down) shouldn't delay the whole app
      // launching; each reconnect attempt just quietly fails on its own.
      tauri::async_runtime::spawn(async move {
        let configs = crate::mcp::load_persisted_configs().await;
        if !configs.is_empty() {
          crate::mcp::restore_servers(configs).await;
        }
      });

      // Pre-warm camoufox-browser in the background — npm installs it into
      // `~/.viv/camoufox/` if it's not already there, so the first web
      // preview/search doesn't have to wait for a cold npm install.
      tauri::async_runtime::spawn(async move {
        crate::camoufox::prewarm().await;
      });

      // Auto-start llama-server instances with autoStart=true from settings.
      let app_clone = app.handle().clone();
      tauri::async_runtime::spawn(async move {
        // Read settings to find llama endpoints with autoStart
        let settings_path = crate::agent::paths::viv_agent_dir()
          .ok()
          .map(|p| p.join("settings.json"));
        let endpoints = if let Some(path) = settings_path {
          if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let settings: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
            settings
              .get("localEndpoints")
              .and_then(|v| v.as_array())
              .map(|arr| {
                arr.iter()
                  .filter(|ep| {
                    ep.get("kind")
                      .and_then(|k| k.as_str())
                      == Some("llama")
                  })
                  .filter_map(|ep| {
                    let config = ep.get("config")?;
                    let auto_start = config.get("autoStart").and_then(|v| v.as_bool()).unwrap_or(false);
                    // Predates the `engine` field — absent means llama.cpp,
                    // same fallback `LlamaProcessConfig::engine`'s
                    // `#[serde(default)]` uses everywhere else.
                    let engine = config
                      .get("engine")
                      .and_then(|v| serde_json::from_value::<crate::agent::llama_manager::LocalEngine>(v.clone()).ok())
                      .unwrap_or_default();
                    let tuning = config
                      .get("tuning")
                      .and_then(|v| serde_json::from_value::<crate::agent::llama_manager::LlamaTuning>(v.clone()).ok())
                      .unwrap_or_default();
                    Some((
                      config.get("id")?.as_str()?.to_string(),
                      config.get("name")?.as_str()?.to_string(),
                      config.get("modelPath")?.as_str()?.to_string(),
                      config.get("port")?.as_u64()? as u16,
                      config.get("ngl")?.as_u64()? as u32,
                      config.get("ctxSize")?.as_u64()? as u32,
                      auto_start,
                      engine,
                      tuning,
                    ))
                  })
                  .collect::<Vec<_>>()
              })
              .unwrap_or_default()
          } else {
            Vec::new()
          }
        } else {
          Vec::new()
        };

        // Every persisted endpoint gets re-registered into the in-memory
        // manager on launch, regardless of `autoStart` — otherwise a model
        // added in a previous session simply vanishes from `cmd_llama_list`
        // on restart even though `settings.json` still has it (the manager
        // is a fresh, empty in-process registry every launch; `settings.json`
        // is the only thing that actually persists). Only `autoStart` ones
        // also get their process launched.
        for (id, name, model_path, port, ngl, ctx_size, auto_start, engine, tuning) in endpoints {
          let manager = app_clone.state::<Arc<LlamaManager>>();
          manager.register(crate::agent::llama_manager::LlamaProcessConfig {
            id: id.clone(),
            name: name.clone(),
            model_path: model_path.clone(),
            port,
            ngl,
            ctx_size,
            auto_start,
            engine,
            tuning,
          });
          if auto_start {
            log::info!("[llama] auto-starting '{}' ({}:{})", name, model_path, port);
            match manager.start(&id, &app_clone).await {
              Ok(status) => log::info!("[llama] '{}' auto-started: {:?}", name, status),
              Err(e) => log::warn!("[llama] failed to auto-start '{}': {}", name, e),
            }
          }
        }
      });

      // Register Viv Instant's global wake chord. The handler above routes it
      // to `instant::window::toggle`.
      {
        use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
        // Command on macOS, Control elsewhere — see `INSTANT_SHORTCUT_LABEL`.
        let primary = if cfg!(target_os = "macos") { Modifiers::SUPER } else { Modifiers::CONTROL };
        let shortcut = Shortcut::new(Some(primary | Modifiers::SHIFT), Code::Space);
        if let Err(e) = app.global_shortcut().register(shortcut) {
          log::warn!("[instant] failed to register global shortcut {INSTANT_SHORTCUT_LABEL}: {e}");
          if let Ok(mut slot) = INSTANT_SHORTCUT_ERROR.lock() {
            *slot = Some(e.to_string());
          }
        }
      }

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            // The multiplayer transport logs every UDP packet at INFO — dozens a
            // second while paired, drowning out everything else in the log.
            .level_for("iroh", log::LevelFilter::Warn)
            .level_for("iroh_quinn", log::LevelFilter::Warn)
            .level_for("iroh_relay", log::LevelFilter::Warn)
            .level_for("netwatch", log::LevelFilter::Warn)
            .level_for("portmapper", log::LevelFilter::Warn)
            .build(),
        )?;
      }

      // Keep the configured default window size from exceeding the screen.
      //
      // tauri.conf.json's width/height are a single fixed pair applied on every
      // machine, and nothing here persists a size between runs, so that literal
      // is what every launch gets. 1280x820 is a good default on a normal
      // display and taller than the usable area of a 1366x768 laptop, where the
      // window would open with its bottom edge (and the composer) off-screen.
      //
      // Tauri exposes the monitor's full bounds but not its work area, so the
      // taskbar/dock is approximated by a margin rather than measured. Only ever
      // shrinks: a screen large enough for the configured size is left alone.
      if let Some(main) = app.get_window("main") {
        if let Ok(Some(monitor)) = main.current_monitor() {
          let scale = monitor.scale_factor();
          let screen = monitor.size().to_logical::<f64>(scale);
          // ~8% vertical allowance for a taskbar; horizontal is generous since
          // side docks are rarely the constraint.
          let max_w = screen.width * 0.96;
          let max_h = screen.height * 0.92;
          if let Ok(current) = main.inner_size() {
            let cur = current.to_logical::<f64>(scale);
            let (w, h) = (cur.width.min(max_w), cur.height.min(max_h));
            if w < cur.width || h < cur.height {
              log::info!("[window] clamping {}x{} to {w:.0}x{h:.0} for a {:.0}x{:.0} screen",
                cur.width, cur.height, screen.width, screen.height);
              let _ = main.set_size(tauri::LogicalSize::new(w, h));
              // Re-centre: shrinking keeps the original top-left, which after a
              // centred layout leaves the window sitting low and right.
              let _ = main.center();
            }
          }
        }
      }

      // The overlay windows (DIP, Viv Instant, pointer) aren't created here:
      // `crate::overlays` builds each on first use, click-through where needed.

      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app_handle, event| {
      // Nothing else runs an exit hook, so nothing else flushed the usage
      // buffer or tore down child processes on quit: the usage buffer's
      // last up-to-5s of unflushed data was silently lost, and every open
      // terminal shell / running dev server / running local model was
      // orphaned instead of exiting with the app.
      if let tauri::RunEvent::Exit = event {
        tauri::async_runtime::block_on(async {
          crate::agent::usage_store::flush_all().await;
          if let Some(terminals) = app_handle.try_state::<Arc<crate::terminal::TerminalRegistry>>() {
            terminals.close_all();
          }
          if let Some(dev_servers) = app_handle.try_state::<Arc<DevServerManager>>() {
            dev_servers.stop_all();
          }
          if let Some(llama) = app_handle.try_state::<Arc<LlamaManager>>() {
            llama.stop_all().await;
          }
        });
      }
    });
}
