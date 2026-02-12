// PDF.dk Desktop - Main library
// Watched folders for automatic PDF processing

mod api;
mod auth;
mod config;
mod processor;
mod watcher;

use config::AppConfig;
use std::sync::{Arc, Mutex};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime, AppHandle,
};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::RwLock;
use tracing::{error, info};
use once_cell::sync::Lazy;

// Global log buffer for debug viewing in the app
static LOG_BUFFER: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Add a log message to the buffer (callable from anywhere)
pub fn add_log(message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_entry = format!("[{}] {}", timestamp, message);

    // Also print to console
    println!("{}", log_entry);

    if let Ok(mut logs) = LOG_BUFFER.lock() {
        logs.push(log_entry);
        // Keep only last 500 logs
        if logs.len() > 500 {
            logs.remove(0);
        }
    }
}

// App state shared across the application
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub auth: Arc<RwLock<auth::AuthState>>,
    pub watcher: Arc<RwLock<Option<watcher::FolderWatcher>>>,
}

// Tauri commands exposed to the frontend

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
async fn save_config(
    state: tauri::State<'_, AppState>,
    new_config: AppConfig,
) -> Result<(), String> {
    let mut config = state.config.write().await;
    *config = new_config.clone();
    config::save_config(&new_config).map_err(|e| e.to_string())?;

    // Restart watcher with new config
    let mut watcher = state.watcher.write().await;
    if let Some(w) = watcher.take() {
        drop(w);
    }
    // Will be restarted by the watcher manager

    Ok(())
}

#[tauri::command]
async fn get_auth_state(state: tauri::State<'_, AppState>) -> Result<auth::AuthState, String> {
    let auth = state.auth.read().await;
    Ok(auth.clone())
}

#[tauri::command]
async fn login(
    state: tauri::State<'_, AppState>,
    email: String,
    password: String,
    remember: Option<bool>,
) -> Result<auth::AuthState, String> {
    let mut result = auth::login(&email, &password).await.map_err(|e| e.to_string())?;

    // All users can login - plan limits are enforced per-file
    // Fetch usage status to get plan limits
    if let Some(ref token) = result.token {
        let client = api::PdfDkClient::new(Some(token.clone()));
        if let Ok(usage) = client.get_usage_status().await {
            result.plan = Some(usage.plan);
            result.jobs_limit = Some(usage.limit);
            result.jobs_used = Some(usage.used);
            result.jobs_remaining = Some(usage.limit - usage.used);
            result.max_file_size_mb = Some(500); // Default, could be fetched from API
            result.is_unlimited = Some(usage.is_unlimited);
        }
    }

    let mut auth_state = state.auth.write().await;
    *auth_state = result.clone();

    // Save token securely
    auth::save_token(&result.token.clone().unwrap_or_default())
        .map_err(|e| e.to_string())?;

    // Save credentials if "Remember me" is checked
    info!("Remember me: {:?}", remember);
    if remember.unwrap_or(false) {
        info!("Saving credentials for {}", email);
        match auth::save_credentials(&email, &password) {
            Ok(_) => info!("Credentials saved successfully"),
            Err(e) => error!("Failed to save credentials: {}", e),
        }
    } else {
        // Clear any previously saved credentials
        let _ = auth::clear_credentials();
    }

    Ok(result)
}

#[tauri::command]
async fn get_saved_credentials() -> Result<Option<serde_json::Value>, String> {
    match auth::load_credentials() {
        Ok((email, password)) => Ok(Some(serde_json::json!({
            "email": email,
            "password": password
        }))),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
async fn logout(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut auth_state = state.auth.write().await;
    *auth_state = auth::AuthState::default();
    auth::clear_token().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn check_auth(state: tauri::State<'_, AppState>) -> Result<auth::AuthState, String> {
    // Try to load saved token and validate it
    if let Ok(token) = auth::load_token() {
        if let Ok(mut auth_result) = auth::validate_token(&token).await {
            // Fetch usage status to get plan limits
            let client = api::PdfDkClient::new(Some(token.clone()));
            if let Ok(usage) = client.get_usage_status().await {
                auth_result.plan = Some(usage.plan);
                auth_result.jobs_limit = Some(usage.limit);
                auth_result.jobs_used = Some(usage.used);
                auth_result.jobs_remaining = Some(usage.limit - usage.used);
                auth_result.max_file_size_mb = Some(500); // Default
                auth_result.is_unlimited = Some(usage.is_unlimited);
            }

            let mut auth_state = state.auth.write().await;
            *auth_state = auth_result.clone();
            return Ok(auth_result);
        }
    }
    Ok(auth::AuthState::default())
}

#[tauri::command]
async fn get_available_tools() -> Result<Vec<config::ToolDefinition>, String> {
    Ok(config::get_available_tools())
}

#[tauri::command]
async fn enable_tool(
    state: tauri::State<'_, AppState>,
    tool_id: String,
    folder_path: String,
) -> Result<(), String> {
    // Update config
    let tool_config = {
        let mut config = state.config.write().await;
        config.enable_tool(&tool_id, &folder_path).map_err(|e| e.to_string())?;
        config::save_config(&config).map_err(|e| e.to_string())?;
        config.tools.iter().find(|t| t.id == tool_id).cloned()
    };

    // Start/update watcher for this tool
    if let Some(tc) = tool_config {
        let mut watcher_guard = state.watcher.write().await;

        // Create watcher if it doesn't exist
        if watcher_guard.is_none() {
            match watcher::FolderWatcher::new() {
                Ok((watcher, mut rx)) => {
                    // Spawn event processor - notifications handled in start_watchers
                    let auth_state = state.auth.clone();
                    tokio::spawn(async move {
                        while let Ok(event) = rx.recv().await {
                            let file_name = event.path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                                .to_string();
                            info!("Processing file: {}", file_name);
                            let token = {
                                let auth = auth_state.read().await;
                                auth.token.clone()
                            };

                            match watcher::process_file_event(event.clone(), token).await {
                                Ok(output_path) => {
                                    add_log(&format!("SUCCESS: {} processed to {:?}", file_name, output_path));
                                }
                                Err(e) => {
                                    add_log(&format!("ERROR: {} failed: {}", file_name, e));
                                }
                            }
                        }
                    });
                    *watcher_guard = Some(watcher);
                }
                Err(e) => {
                    error!("Failed to create watcher: {}", e);
                    return Err(format!("Failed to create file watcher: {}", e));
                }
            }
        }

        // Add folder to watcher
        if let Some(watcher) = watcher_guard.as_mut() {
            watcher.add_folder(tc).await.map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
async fn disable_tool(state: tauri::State<'_, AppState>, tool_id: String) -> Result<(), String> {
    // Get the folder path before disabling
    let folder_path = {
        let config = state.config.read().await;
        config.tools.iter()
            .find(|t| t.id == tool_id)
            .and_then(|t| t.folder_path.clone())
            .map(std::path::PathBuf::from)
    };

    // Update config
    {
        let mut config = state.config.write().await;
        config.disable_tool(&tool_id);
        config::save_config(&config).map_err(|e| e.to_string())?;
    }

    // Remove folder from watcher
    if let Some(path) = folder_path {
        let mut watcher_guard = state.watcher.write().await;
        if let Some(watcher) = watcher_guard.as_mut() {
            let _ = watcher.remove_folder(&path).await;
        }
    }

    Ok(())
}

#[tauri::command]
async fn get_jobs(_state: tauri::State<'_, AppState>) -> Result<Vec<processor::Job>, String> {
    // Return recent jobs from processor
    Ok(vec![]) // TODO: implement job tracking
}

#[tauri::command]
async fn start_watchers(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    info!("Starting watchers for enabled tools...");

    // Get enabled tools from config
    let enabled_tools: Vec<config::ToolConfig> = {
        let config = state.config.read().await;
        config.tools.iter()
            .filter(|t| t.enabled && t.folder_path.is_some())
            .cloned()
            .collect()
    };

    if enabled_tools.is_empty() {
        add_log("No enabled tools to watch");
        return Ok(());
    }

    add_log(&format!("Found {} enabled tools to watch", enabled_tools.len()));

    let mut watcher_guard = state.watcher.write().await;

    // Create watcher if it doesn't exist
    if watcher_guard.is_none() {
        add_log("Creating new file watcher...");
        match watcher::FolderWatcher::new() {
            Ok((watcher, mut rx)) => {
                add_log("File watcher created successfully");
                // Spawn event processor
                let auth_state = state.auth.clone();
                let app_handle = app.clone();
                tokio::spawn(async move {
                    add_log("Event receiver task started - waiting for files...");
                    while let Ok(event) = rx.recv().await {
                        let file_name = event.path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file")
                            .to_string();
                        add_log(&format!("Received file event: {} for tool: {}", file_name, event.tool_id));
                        let token = {
                            let auth = auth_state.read().await;
                            auth.token.clone()
                        };

                        add_log(&format!("Processing file with tool: {}", event.tool_id));
                        match watcher::process_file_event(event.clone(), token).await {
                            Ok(output_path) => {
                                add_log(&format!("SUCCESS: File processed to {:?}", output_path));
                                // Send success notification
                                let _ = app_handle.notification()
                                    .builder()
                                    .title("PDF.dk - File Processed")
                                    .body(&format!("{} completed successfully", file_name))
                                    .show();
                            }
                            Err(e) => {
                                let error_msg = format!("{}", e);
                                add_log(&format!("ERROR: Failed to process file: {}", error_msg));
                                // Send error notification
                                let _ = app_handle.notification()
                                    .builder()
                                    .title("PDF.dk - Processing Failed")
                                    .body(&format!("{}: {}", file_name, error_msg))
                                    .show();
                            }
                        }
                    }
                    add_log("Event receiver task ended");
                });
                *watcher_guard = Some(watcher);
            }
            Err(e) => {
                add_log(&format!("ERROR: Failed to create watcher: {}", e));
                return Err(format!("Failed to create file watcher: {}", e));
            }
        }
    }

    // Add all enabled tool folders to watcher
    if let Some(watcher) = watcher_guard.as_mut() {
        for tool in enabled_tools {
            add_log(&format!("Adding watch folder for tool: {} at {:?}", tool.id, tool.folder_path));
            if let Err(e) = watcher.add_folder(tool.clone()).await {
                add_log(&format!("ERROR: Failed to add folder for tool {}: {}", tool.id, e));
            }
        }
    }

    add_log("Watcher setup complete");
    Ok(())
}

#[tauri::command]
async fn select_folder() -> Result<Option<String>, String> {
    // This will be handled by tauri-plugin-dialog on frontend
    Ok(None)
}

#[tauri::command]
async fn update_tool_options(
    state: tauri::State<'_, AppState>,
    tool_id: String,
    options: serde_json::Value,
) -> Result<(), String> {
    let mut config = state.config.write().await;

    // Find the tool index first
    let tool_idx = config.tools.iter().position(|t| t.id == tool_id);

    if let Some(idx) = tool_idx {
        config.tools[idx].options = options.clone();
        config::save_config(&config).map_err(|e| e.to_string())?;
        info!("Updated options for tool {}: {:?}", tool_id, options);
    } else {
        return Err(format!("Tool not found: {}", tool_id));
    }

    Ok(())
}

#[tauri::command]
fn get_logs() -> Vec<String> {
    LOG_BUFFER.lock().map(|logs| logs.clone()).unwrap_or_default()
}

#[tauri::command]
fn clear_logs() {
    if let Ok(mut logs) = LOG_BUFFER.lock() {
        logs.clear();
    }
}

fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    // Get the existing tray icon created by Tauri from tauri.conf.json
    let tray = app.tray_by_id("main").ok_or("Tray not found")?;

    // Create menu
    let show = tauri::menu::MenuItem::with_id(app, "show", "Show PDF.dk Desktop", true, None::<&str>)?;
    let pause = tauri::menu::MenuItem::with_id(app, "pause", "Pause Processing", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show, &pause, &quit])?;

    // Set menu on existing tray
    tray.set_menu(Some(menu))?;
    tray.set_show_menu_on_left_click(false)?;

    // Set up menu event handler
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "show" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "pause" => {
            info!("Pause processing requested");
            // TODO: Toggle pause state
        }
        "quit" => {
            info!("Quit requested");
            app.exit(0);
        }
        _ => {}
    });

    // Set up click event handler
    tray.on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let app = tray.app_handle();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Load config
            let config = config::load_config().unwrap_or_default();

            // Initialize app state
            let state = AppState {
                config: Arc::new(RwLock::new(config)),
                auth: Arc::new(RwLock::new(auth::AuthState::default())),
                watcher: Arc::new(RwLock::new(None)),
            };

            app.manage(state);

            // Setup system tray
            if let Err(e) = setup_tray(app) {
                error!("Failed to setup tray: {}", e);
            }

            // Handle window close - hide to tray instead of quitting
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Prevent the window from closing
                        api.prevent_close();
                        // Hide the window instead - it stays in the system tray
                        let _ = window_clone.hide();
                        info!("Window hidden to tray");
                    }
                });
            }

            // Start watching folders (will be done after auth check in frontend)
            info!("PDF.dk Desktop started");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_auth_state,
            login,
            logout,
            check_auth,
            get_available_tools,
            enable_tool,
            disable_tool,
            get_jobs,
            select_folder,
            start_watchers,
            get_saved_credentials,
            update_tool_options,
            get_logs,
            clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
