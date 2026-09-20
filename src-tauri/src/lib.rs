// PDF.dk Desktop - Main library
// Watched folders, "open with", drag-and-drop: every file goes through the
// website's API with the tool the user chose.

mod api;
mod auth;
mod config;
mod processor;
mod watcher;

use config::{AppConfig, ToolConfig, ToolDefinition};
use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, Runtime,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::RwLock;
use tracing::{error, info};

// ------------------------------------------------------------ log buffer

static LOG_BUFFER: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn add_log(message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_entry = format!("[{}] {}", timestamp, message);
    println!("{}", log_entry);
    if let Ok(mut logs) = LOG_BUFFER.lock() {
        logs.push(log_entry);
        if logs.len() > 500 {
            logs.remove(0);
        }
    }
}

// ------------------------------------------------------------- job list

static JOBS: Lazy<Mutex<Vec<processor::Job>>> = Lazy::new(|| Mutex::new(Vec::new()));

fn job_add(job: processor::Job) -> String {
    let id = job.id.clone();
    if let Ok(mut jobs) = JOBS.lock() {
        jobs.insert(0, job);
        jobs.truncate(200);
    }
    id
}

fn job_update(id: &str, f: impl FnOnce(&mut processor::Job)) {
    if let Ok(mut jobs) = JOBS.lock() {
        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }
}

// ------------------------------------------------------------- app state

pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub auth: Arc<RwLock<auth::AuthState>>,
    pub watcher: Arc<RwLock<Option<watcher::FolderWatcher>>>,
    pub catalog: Arc<RwLock<Vec<ToolDefinition>>>,
    /// files handed to us by the OS before the window was ready
    pub pending_files: Mutex<Vec<String>>,
}

fn notify<R: Runtime>(app: &AppHandle<R>, cfg: &Arc<RwLock<AppConfig>>, title: &str, body: &str) {
    let enabled = cfg.try_read().map(|c| c.general.show_notifications).unwrap_or(true);
    if enabled {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

fn tool_label(tc: &ToolConfig, lang: &str) -> String {
    match &tc.name {
        Some(b) if lang == "en" && !b.en.is_empty() => b.en.clone(),
        Some(b) if !b.da.is_empty() => b.da.clone(),
        _ => tc.id.clone(),
    }
}

/// Consume watcher events: run the job, track it, notify.
fn spawn_event_loop(app: AppHandle, state_auth: Arc<RwLock<auth::AuthState>>, state_cfg: Arc<RwLock<AppConfig>>, mut rx: tokio::sync::broadcast::Receiver<watcher::FileEvent>) {
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let file_name = event.path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
            let lang = state_cfg.read().await.general.language.clone();
            let label = tool_label(&event.tool_config, &lang);
            add_log(&format!("{} → {}", file_name, label));

            let job_id = job_add(processor::Job::new(&event.tool_id, &label, &event.path.to_string_lossy()));
            job_update(&job_id, |j| j.set_processing());

            let token = state_auth.read().await.token.clone();
            match watcher::process_file_event(event.clone(), token).await {
                Ok(output_path) => {
                    add_log(&format!("OK: {} → {:?}", file_name, output_path));
                    job_update(&job_id, |j| j.set_completed(&output_path.to_string_lossy()));
                    notify(&app, &state_cfg, &format!("{} · PDF.dk", label), &if lang == "en" { format!("{} is done", file_name) } else { format!("{} er færdig", file_name) });
                }
                Err(e) => {
                    let msg = e.to_string();
                    add_log(&format!("FAILED: {} — {}", file_name, msg));
                    job_update(&job_id, |j| j.set_failed(&msg));
                    notify(&app, &state_cfg, &format!("{} · PDF.dk", label), &format!("{}: {}", file_name, msg));
                }
            }
        }
    });
}

async fn ensure_watcher(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut guard = state.watcher.write().await;
    if guard.is_none() {
        let (w, rx) = watcher::FolderWatcher::new().map_err(|e| format!("Failed to create file watcher: {}", e))?;
        spawn_event_loop(app.clone(), state.auth.clone(), state.config.clone(), rx);
        *guard = Some(w);
    }
    Ok(())
}

// -------------------------------------------------------------- commands

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
async fn save_config(app: AppHandle, state: tauri::State<'_, AppState>, new_config: AppConfig) -> Result<(), String> {
    {
        let mut config = state.config.write().await;
        *config = new_config.clone();
    }
    config::save_config(&new_config).map_err(|e| e.to_string())?;
    // start-at-login follows the setting
    let al = app.autolaunch();
    let res = if new_config.general.start_on_login { al.enable() } else { al.disable() };
    if let Err(e) = res {
        add_log(&format!("autostart: {}", e));
    }
    Ok(())
}

#[tauri::command]
async fn get_auth_state(state: tauri::State<'_, AppState>) -> Result<auth::AuthState, String> {
    Ok(state.auth.read().await.clone())
}

async fn fill_usage(result: &mut auth::AuthState) {
    if let Some(ref token) = result.token {
        let client = api::PdfDkClient::new(Some(token.clone()));
        if let Ok(usage) = client.get_usage_status().await {
            result.apply_plan(&usage.plan);
            result.jobs_limit = Some(usage.limit);
            result.jobs_used = Some(usage.used);
            result.jobs_remaining = Some(usage.limit - usage.used);
            result.max_file_size_mb = usage.max_file_size_mb.or(Some(100));
            result.is_unlimited = Some(usage.is_unlimited);
        }
    }
}

#[tauri::command]
async fn login(state: tauri::State<'_, AppState>, email: String, password: String, remember: Option<bool>) -> Result<auth::AuthState, String> {
    let mut result = auth::login(&email, &password).await.map_err(|e| e.to_string())?;
    fill_usage(&mut result).await;
    {
        let mut auth_state = state.auth.write().await;
        *auth_state = result.clone();
    }
    auth::save_token(&result.token.clone().unwrap_or_default()).map_err(|e| e.to_string())?;
    if remember.unwrap_or(false) {
        if let Err(e) = auth::save_credentials(&email, &password) {
            error!("Failed to save credentials: {}", e);
        }
    } else {
        let _ = auth::clear_credentials();
    }
    Ok(result)
}

#[tauri::command]
async fn get_saved_credentials() -> Result<Option<serde_json::Value>, String> {
    match auth::load_credentials() {
        Ok((email, password)) => Ok(Some(serde_json::json!({ "email": email, "password": password }))),
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
    if let Ok(token) = auth::load_token() {
        match auth::validate_token(&token).await {
            Ok(mut result) => {
                fill_usage(&mut result).await;
                let mut auth_state = state.auth.write().await;
                *auth_state = result.clone();
                return Ok(result);
            }
            Err(e) => add_log(&format!("Saved session not valid: {}", e)),
        }
    }
    Ok(auth::AuthState::default())
}

/// The tools catalogue: live from the API, else the cached copy, else the
/// two built-in tools. Never empty.
#[tauri::command]
async fn get_available_tools(state: tauri::State<'_, AppState>, refresh: Option<bool>) -> Result<Vec<ToolDefinition>, String> {
    if !refresh.unwrap_or(false) {
        let current = state.catalog.read().await;
        if !current.is_empty() {
            return Ok(current.clone());
        }
    }
    let token = state.auth.read().await.token.clone();
    let client = api::PdfDkClient::new(token);
    let tools = match client.fetch_catalog().await {
        Ok(cache) => {
            add_log(&format!("Tools catalogue {} loaded: {} tools", cache.version.clone().unwrap_or_default(), cache.tools.len()));
            if let Err(e) = config::save_cached_catalog(&cache) {
                add_log(&format!("Could not cache catalogue: {}", e));
            }
            cache.tools
        }
        Err(e) => {
            add_log(&format!("Catalogue fetch failed ({}), using cached copy", e));
            match config::load_cached_catalog() {
                Some(c) if !c.tools.is_empty() => c.tools,
                _ => config::builtin_catalog(),
            }
        }
    };
    let mut cat = state.catalog.write().await;
    *cat = tools.clone();
    Ok(tools)
}

async fn find_tool(state: &AppState, tool_id: &str) -> Option<ToolDefinition> {
    let cat = state.catalog.read().await;
    cat.iter().find(|t| t.id == tool_id).cloned()
}

#[tauri::command]
async fn enable_tool(app: AppHandle, state: tauri::State<'_, AppState>, tool_id: String, folder_path: String, key: Option<String>) -> Result<(), String> {
    let def = find_tool(&state, &tool_id).await.ok_or_else(|| format!("Unknown tool: {}", tool_id))?;
    // changing an existing entry's folder: stop watching the old one first
    let old_folder = match key.as_deref() {
        Some(k) => state.config.read().await.tools.iter().find(|t| t.key == k).and_then(|t| t.folder_path.clone()).map(PathBuf::from),
        None => None,
    };
    let tool_config = {
        let mut config = state.config.write().await;
        let k = config.enable_tool(&def, &folder_path, key.as_deref()).map_err(|e| e.to_string())?;
        config::save_config(&config).map_err(|e| e.to_string())?;
        config.tools.iter().find(|t| t.key == k).cloned()
    };
    if let Some(tc) = tool_config {
        ensure_watcher(&app, &state).await?;
        let mut guard = state.watcher.write().await;
        if let Some(w) = guard.as_mut() {
            if let Some(old) = old_folder.filter(|o| o.as_os_str() != std::ffi::OsStr::new(&folder_path)) {
                let _ = w.remove_folder(&old).await;
            }
            w.add_folder(tc).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn disable_tool(state: tauri::State<'_, AppState>, key: String) -> Result<(), String> {
    let folder_path = {
        let mut config = state.config.write().await;
        let f = config.disable_tool(&key);
        config::save_config(&config).map_err(|e| e.to_string())?;
        f
    };
    if let Some(path) = folder_path {
        let mut guard = state.watcher.write().await;
        if let Some(w) = guard.as_mut() {
            let _ = w.remove_folder(&path).await;
        }
    }
    Ok(())
}

/// `key` = one watched folder's options; without it the tool's first entry
/// (also what drag-and-drop / Open with use), created disabled if needed.
#[tauri::command]
async fn update_tool_options(state: tauri::State<'_, AppState>, tool_id: String, options: serde_json::Value, key: Option<String>) -> Result<(), String> {
    let mut config = state.config.write().await;
    let found = match key.as_deref() {
        Some(k) => config.tools.iter_mut().find(|t| t.key == k),
        None => config.tools.iter_mut().find(|t| t.id == tool_id),
    };
    if let Some(tc) = found {
        tc.options = options;
    } else if key.is_some() {
        return Err("Unknown folder entry".to_string());
    } else {
        let def = { let cat = state.catalog.read().await; cat.iter().find(|t| t.id == tool_id).cloned() };
        let Some(def) = def else { return Err(format!("Unknown tool: {}", tool_id)) };
        config.tools.push(ToolConfig {
            id: def.id.clone(), key: def.id.clone(), enabled: false, folder_path: None, output_mode: config::OutputMode::Subfolder,
            options, endpoint: Some(def.endpoint.clone()), file_field: Some(def.file_field.clone()),
            accepts: def.accepts.clone(), output: Some(def.output.clone()), name: Some(def.name.clone()),
        });
    }
    config::save_config(&config).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn start_watchers(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let enabled: Vec<ToolConfig> = {
        let config = state.config.read().await;
        config.tools.iter().filter(|t| t.enabled && t.folder_path.is_some()).cloned().collect()
    };
    if enabled.is_empty() {
        return Ok(());
    }
    ensure_watcher(&app, &state).await?;
    let mut guard = state.watcher.write().await;
    if let Some(w) = guard.as_mut() {
        for tool in enabled {
            if let Err(e) = w.add_folder(tool.clone()).await {
                add_log(&format!("Could not watch folder for {}: {}", tool.id, e));
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileResult {
    input: String,
    output: Option<String>,
    error: Option<String>,
}

/// Run files the user dropped on the window / opened with the app through
/// one tool. Output lands next to the source; the original stays put.
#[tauri::command]
async fn process_files(app: AppHandle, state: tauri::State<'_, AppState>, tool_id: String, paths: Vec<String>) -> Result<Vec<FileResult>, String> {
    let def = find_tool(&state, &tool_id).await.ok_or_else(|| format!("Unknown tool: {}", tool_id))?;
    let (tc, lang) = {
        let config = state.config.read().await;
        let saved = config.tools.iter().find(|t| t.id == tool_id).cloned();
        let mut options = def.default_options();
        if let Some(s) = saved {
            if let (Some(base), Some(over)) = (options.as_object_mut(), s.options.as_object()) {
                for (k, v) in over { base.insert(k.clone(), v.clone()); }
            }
        }
        (ToolConfig {
            id: def.id.clone(),
            key: def.id.clone(), enabled: true, folder_path: None, output_mode: config::OutputMode::SameFolder,
            options, endpoint: Some(def.endpoint.clone()), file_field: Some(def.file_field.clone()),
            accepts: def.accepts.clone(), output: Some(def.output.clone()), name: Some(def.name.clone()),
        }, config.general.language.clone())
    };
    let label = tool_label(&tc, &lang);
    let token = state.auth.read().await.token.clone();
    let mut results = Vec::new();
    for p in paths {
        let path = PathBuf::from(&p);
        let job_id = job_add(processor::Job::new(&tc.id, &label, &p));
        job_update(&job_id, |j| j.set_processing());
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        match watcher::run_tool(&path, &tc, token.clone(), Some(&parent)).await {
            Ok(out) => {
                job_update(&job_id, |j| j.set_completed(&out.to_string_lossy()));
                results.push(FileResult { input: p, output: Some(out.to_string_lossy().to_string()), error: None });
            }
            Err(e) => {
                job_update(&job_id, |j| j.set_failed(&e.to_string()));
                results.push(FileResult { input: p, output: None, error: Some(e.to_string()) });
            }
        }
    }
    let ok = results.iter().filter(|r| r.output.is_some()).count();
    notify(&app, &state.config, &format!("{} · PDF.dk", label), &if lang == "en" { format!("{} of {} files done", ok, results.len()) } else { format!("{} af {} filer færdige", ok, results.len()) });
    Ok(results)
}

#[tauri::command]
fn take_pending_files(state: tauri::State<'_, AppState>) -> Vec<String> {
    state.pending_files.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

#[tauri::command]
fn get_jobs() -> Vec<processor::Job> {
    JOBS.lock().map(|j| j.clone()).unwrap_or_default()
}

#[tauri::command]
fn get_site_base() -> String {
    api::site_base()
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

// ------------------------------------------------------------------ tray

fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let show = tauri::menu::MenuItem::with_id(app, "show", "Vis PDF.dk Desktop", true, None::<&str>)?;
    let site = tauri::menu::MenuItem::with_id(app, "site", "Åbn pdf.dk", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", "Afslut", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show, &site, &quit])?;

    let tray = match app.tray_by_id("main") {
        Some(t) => t,
        None => TrayIconBuilder::with_id("main").icon(app.default_window_icon().cloned().ok_or("no icon")?).build(app)?,
    };
    tray.set_menu(Some(menu))?;
    tray.set_show_menu_on_left_click(false)?;

    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "show" => show_main(app),
        "site" => {
            let _ = tauri_plugin_opener::open_url(api::site_base(), None::<&str>);
        }
        "quit" => app.exit(0),
        _ => {}
    });
    tray.on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            show_main(tray.app_handle());
        }
    });
    Ok(())
}

fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Files handed to us by the OS (dock drop, "Open with", command line).
fn deliver_files<R: Runtime>(app: &AppHandle<R>, paths: Vec<String>) {
    let paths: Vec<String> = paths.into_iter().filter(|p| Path::new(p).is_file()).collect();
    if paths.is_empty() {
        return;
    }
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut pending) = state.pending_files.lock() {
            pending.extend(paths.clone());
        }
    }
    show_main(app);
    let _ = app.emit("files-opened", paths);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    let cli_files: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-') && Path::new(a).is_file()).collect();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(move |app| {
            let config = config::load_config().unwrap_or_default();
            let start_minimized = config.general.start_minimized && std::env::args().any(|a| a == "--minimized");
            let cached = config::load_cached_catalog().map(|c| c.tools).unwrap_or_default();

            app.manage(AppState {
                config: Arc::new(RwLock::new(config)),
                auth: Arc::new(RwLock::new(auth::AuthState::default())),
                watcher: Arc::new(RwLock::new(None)),
                catalog: Arc::new(RwLock::new(cached)),
                pending_files: Mutex::new(cli_files.clone()),
            });

            if let Err(e) = setup_tray(app) {
                error!("Failed to setup tray: {}", e);
            }

            if let Some(window) = app.get_webview_window("main") {
                if start_minimized && cli_files.is_empty() {
                    let _ = window.hide();
                }
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                });
            }

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
            update_tool_options,
            start_watchers,
            process_files,
            take_pending_files,
            get_jobs,
            get_site_base,
            get_saved_credentials,
            get_logs,
            clear_logs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = &event {
            let paths: Vec<String> = urls.iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.to_string_lossy().to_string()).collect();
            deliver_files(app_handle, paths);
        }
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = &event {
            show_main(app_handle);
        }
        let _ = (&event, app_handle);
    });
}
