use std::sync::Arc;

use tauri::{Emitter, Manager};

pub mod ai;
use crate::ai::auth::{AuthState, auth_read_credential, auth_store_api_key, auth_store_oauth, auth_delete_credential, auth_is_authenticated, auth_resolve_api_key};
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
use crate::automation::AutomationStore;
use crate::agent::llama_manager::LlamaManager;

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
      crate::agent::commands::cmd_register_local_endpoint,
      crate::agent::commands::cmd_unregister_local_endpoint,
      crate::agent::commands::cmd_list_local_endpoints,
      crate::agent::llama_manager::cmd_llama_start,
      crate::agent::llama_manager::cmd_llama_stop,
      crate::agent::llama_manager::cmd_llama_status,
      crate::agent::llama_manager::cmd_llama_list,
      crate::agent::llama_manager::cmd_llama_register,
      crate::agent::llama_manager::cmd_llama_unregister,
      crate::agent::commands::cmd_save_settings,
      crate::agent::commands::cmd_get_sessions,
      crate::agent::commands::cmd_get_active_session_id,
      crate::agent::projects::cmd_list_projects,
      crate::agent::projects::cmd_create_project,
      crate::agent::projects::cmd_rename_project,
      crate::agent::projects::cmd_delete_project,
      crate::agent::projects::cmd_get_session_project_map,
      crate::agent::projects::cmd_assign_session_project,
      crate::agent::commands::cmd_delete_session,
      crate::agent::commands::cmd_write_text_file,
      crate::agent::commands::cmd_load_session,
      crate::agent::commands::cmd_save_current_session,
      crate::agent::commands::cmd_rename_session,
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
      // Initialize auth state — OS keychain first (API keys never touch disk
      // in plaintext), with the file-backed store kept only as an explicit,
      // logged fallback for platforms/environments where the OS credential
      // backend isn't reachable (e.g. a headless CI runner).
      let auth_state = tauri::async_runtime::block_on(async {
        if crate::ai::keychain_store::KeychainCredentialStore::is_available() {
          // One-time migration from a legacy plaintext credentials.json, if
          // present — run against a standalone store instance before
          // building the one `AuthState` actually uses; both talk to the
          // same OS keychain, so there's no shared-state concern here.
          if let Some(dir) = app.path().app_data_dir().ok() {
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
          let app_data_dir = app.path().app_data_dir().ok();
          if let Some(dir) = app_data_dir {
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
      });
      app.manage(auth_state);

      // Initialize app state
      let app_state = Arc::new(AppState::default());
      app.manage(app_state);

      // Initialize local endpoint registry
      app.manage(crate::ai::local_registry::new_registry());

      // Initialize llama-server process manager
      app.manage(Arc::new(LlamaManager::new()));

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
                    let auto_start = config.get("autoStart")?.as_bool().unwrap_or(false);
                    if !auto_start { return None; }
                    Some((
                      config.get("id")?.as_str()?.to_string(),
                      config.get("name")?.as_str()?.to_string(),
                      config.get("modelPath")?.as_str()?.to_string(),
                      config.get("port")?.as_u64()? as u16,
                      config.get("ngl")?.as_u64()? as u32,
                      config.get("ctxSize")?.as_u64()? as u32,
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

        // Register and start each auto-start endpoint
        for (id, name, model_path, port, ngl, ctx_size) in endpoints {
          log::info!("[llama] auto-starting '{}' ({}:{})", name, model_path, port);
          let manager = app_clone.state::<Arc<LlamaManager>>();
          manager.register(crate::agent::llama_manager::LlamaProcessConfig {
            id: id.clone(),
            name: name.clone(),
            model_path: model_path.clone(),
            port,
            ngl,
            ctx_size,
            auto_start: true,
          });
          match manager.start(&id, &app_clone).await {
            Ok(status) => log::info!("[llama] '{}' auto-started: {:?}", name, status),
            Err(e) => log::warn!("[llama] failed to auto-start '{}': {}", name, e),
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
