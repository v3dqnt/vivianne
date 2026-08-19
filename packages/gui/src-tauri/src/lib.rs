use std::sync::Arc;

use tauri::{Emitter, Manager};

pub mod ai;
use crate::ai::auth::{AuthState, auth_read_credential, auth_store_api_key, auth_store_oauth, auth_delete_credential, auth_is_authenticated, auth_resolve_api_key, CredentialPool, cmd_pool_list, cmd_pool_add, cmd_pool_remove, cmd_pool_set_strategy, cmd_pool_get_strategy, cmd_pool_reset_cooldowns};
pub mod agent;
use crate::agent::commands::AppState;

// New modules
pub mod coding_agent;
pub mod orchestrator;
pub mod lsp;
pub mod mcp;
pub mod camoufox;
pub mod docker_installer;
pub mod voice;
pub mod automation;
pub mod terminal;
pub mod gesture;
use crate::automation::AutomationStore;
use crate::agent::llama_manager::LlamaManager;
use crate::agent::dev_server_manager::DevServerManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_store::Builder::default().build())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(
      tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
          // Fire on key-press (not release) and broadcast to every window so
          // the notch (which runs the voice pipeline) force-activates even when
          // the main window isn't focused.
          if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
            let _ = app.emit("voice-wake", ());
          }
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
      crate::agent::projects::cmd_update_project,
      crate::agent::projects::cmd_open_external,
      crate::agent::projects::cmd_delete_project,
      crate::agent::projects::cmd_get_session_project_map,
      crate::agent::projects::cmd_assign_session_project,
      crate::agent::commands::cmd_delete_session,
      crate::agent::commands::cmd_write_text_file,
      crate::agent::commands::cmd_load_session,
      crate::agent::commands::cmd_save_current_session,
      crate::agent::commands::cmd_rename_session,
      crate::agent::commands::cmd_new_session,
      crate::agent::commands::cmd_agent_revert_to_index,
      crate::agent::commands::cmd_get_usage_history,
      crate::agent::commands::cmd_session_cost,
      crate::agent::commands::cmd_list_dir_tree,
      crate::agent::commands::cmd_read_file_content,
      crate::agent::notifications::cmd_test_notification,
      crate::agent::commands::cmd_fork_session,
      crate::agent::commands::cmd_clone_session,
      crate::agent::commands::cmd_list_skills,
      crate::agent::commands::cmd_save_skill_file,
      crate::agent::commands::cmd_list_playbooks,
      crate::agent::commands::cmd_add_playbook_file,
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
      crate::agent::commands::cmd_check_docker_status,
      crate::agent::commands::cmd_install_docker,
      crate::agent::commands::cmd_terminal_list_shells,
      crate::agent::commands::cmd_terminal_spawn,
      crate::agent::commands::cmd_terminal_write,
      crate::agent::commands::cmd_terminal_resize,
      crate::agent::commands::cmd_terminal_close,
      crate::agent::commands::cmd_lsp_diagnostics,
      crate::agent::commands::cmd_git_current_branch,
      crate::agent::commands::cmd_terminal_list_sessions,
      crate::gesture::commands::cmd_gesture_start,
      crate::gesture::commands::cmd_gesture_stop,
      crate::gesture::commands::cmd_gesture_status,
      crate::gesture::commands::cmd_gesture_set_debug,
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
      // Orchestrator commands
      crate::orchestrator::commands::cmd_orch_spawn,
      crate::orchestrator::commands::cmd_orch_list,
      crate::orchestrator::commands::cmd_orch_status,
      crate::orchestrator::commands::cmd_orch_stop,
      crate::orchestrator::commands::cmd_orch_rpc,
      crate::orchestrator::commands::cmd_orch_rpc_stream,
      // Grok-parity coding-agent commands (subagents, plan mode, ask-user)
      crate::coding_agent::commands::cmd_coding_answer_question,
      crate::coding_agent::commands::cmd_coding_list_subagents,
      crate::coding_agent::commands::cmd_coding_get_subagent_messages,
      crate::coding_agent::commands::cmd_coding_kill_subagent,
      crate::coding_agent::commands::cmd_coding_plan_mode,
      crate::coding_agent::commands::cmd_coding_set_plan_mode,
      crate::coding_agent::commands::cmd_coding_register_stream_channel,
      crate::coding_agent::computer::cmd_capture_screen,
      crate::voice::cmd_whisper_preload,
      crate::voice::cmd_whisper_transcribe,
      // Automation (scheduled agent tasks)
      crate::automation::cmd_automation_list,
      crate::automation::cmd_automation_create,
      crate::automation::cmd_automation_update,
      crate::automation::cmd_automation_set_enabled,
      crate::automation::cmd_automation_delete,
      crate::automation::cmd_automation_run_now,
    ])
    .setup(|app| {
      // Abandoned "New chat" threads are persisted on creation, so clear out
      // any that were never used. Safe here specifically: nothing is active
      // at startup.
      let pruned = crate::agent::commands::prune_empty_sessions();
      if pruned > 0 {
        log::info!("[sessions] pruned {pruned} empty session(s)");
      }

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

      // Initialize local endpoint registry
      app.manage(crate::ai::local_registry::new_registry());

      // Initialize llama-server process manager
      app.manage(Arc::new(LlamaManager::new()));

      // Initialize dev-server process manager (Live Preview panel)
      app.manage(Arc::new(DevServerManager::default()));

// Grok-parity: install the ask_user_question emitter so tools can
      // surface a choice UI to the frontend. The frontend listens for the
      // `coding_ask_user_question` event and calls
      // `cmd_coding_answer_question` when the user picks options.
      let emit_handle = app.handle().clone();
      crate::coding_agent::orchestration::install_question_emitter(std::sync::Arc::new(
        move |payload| {
          let _ = emit_handle.emit("coding_ask_user_question", payload);
        },
      ));

      // Grok-parity: streaming_bash / streaming_powershell push live output
      // over a Tauri `Channel` (registered by the frontend via
      // `cmd_coding_register_stream_channel`), not a global `emit()` — see
      // that command's doc comment for why.

      // Initialize orchestrator supervisor
      let orch_storage_path = app.path().app_data_dir().ok().map(|dir| dir.join("instances.json"));
      let orch_storage = crate::orchestrator::storage::InstanceStorage::new(
        orch_storage_path.unwrap_or_default()
      );
      let supervisor = crate::orchestrator::supervisor::Supervisor::new(orch_storage, app.handle().clone());
      let supervisor = std::sync::Arc::new(tokio::sync::Mutex::new(supervisor));
      app.manage(supervisor);

      // Automation store + background scheduler (emits `automation_fired`).
      app.manage(Arc::new(AutomationStore::new()));
      crate::automation::spawn_scheduler(app.handle().clone());

      // PTY-backed terminal sessions (one per open tab).
      app.manage(Arc::new(crate::terminal::TerminalRegistry::new()));
      app.manage(Arc::new(crate::gesture::GestureSidecar::new()));

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
                    Some((
                      config.get("id")?.as_str()?.to_string(),
                      config.get("name")?.as_str()?.to_string(),
                      config.get("modelPath")?.as_str()?.to_string(),
                      config.get("port")?.as_u64()? as u16,
                      config.get("ngl")?.as_u64()? as u32,
                      config.get("ctxSize")?.as_u64()? as u32,
                      auto_start,
                      engine,
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
        for (id, name, model_path, port, ngl, ctx_size, auto_start, engine) in endpoints {
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

      // Register the default global voice-wake shortcut (Ctrl+Shift+V). The
      // handler above broadcasts a "voice-wake" event that VoiceMode listens for.
      {
        use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
        let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyV);
        if let Err(e) = app.global_shortcut().register(shortcut) {
          log::warn!("[voice] failed to register global shortcut Ctrl+Shift+V: {e}");
        }
      }

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
