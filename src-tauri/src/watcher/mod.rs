// File watcher module for PDF.dk Desktop
// Watches folders for new files and hands them to the tool of that folder.

use crate::api::PdfDkClient;
use crate::config::{OutputMode, ToolConfig};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("Notify error: {0}")]
    Notify(#[from] notify::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error")]
    ChannelError,
}

#[derive(Debug, Clone)]
pub struct FileEvent {
    pub path: PathBuf,
    pub tool_id: String,
    pub tool_config: ToolConfig,
}

pub struct FolderWatcher {
    watcher: RecommendedWatcher,
    watched_folders: Arc<RwLock<HashMap<PathBuf, ToolConfig>>>,
    #[allow(dead_code)]
    event_sender: broadcast::Sender<FileEvent>,
}

impl FolderWatcher {
    pub fn new() -> Result<(Self, broadcast::Receiver<FileEvent>), WatcherError> {
        let (event_tx, event_rx) = broadcast::channel(100);
        let (notify_tx, mut notify_rx) = mpsc::channel(100);

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    if let Err(e) = notify_tx.blocking_send(event) {
                        crate::add_log(&format!("Failed to send event to channel: {}", e));
                    }
                }
                Err(e) => crate::add_log(&format!("File watcher error: {}", e)),
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        let watched_folders = Arc::new(RwLock::new(HashMap::new()));
        let folder_watcher = Self {
            watcher,
            watched_folders: watched_folders.clone(),
            event_sender: event_tx.clone(),
        };

        let event_sender = event_tx;
        let wf = watched_folders.clone();
        tokio::spawn(async move {
            Self::process_events(&mut notify_rx, wf, event_sender).await;
        });

        Ok((folder_watcher, event_rx))
    }

    pub async fn add_folder(&mut self, tool_config: ToolConfig) -> Result<(), WatcherError> {
        let folder_path = match &tool_config.folder_path {
            Some(path) => PathBuf::from(path),
            None => return Ok(()),
        };
        if !tool_config.enabled {
            return Ok(());
        }
        if !folder_path.exists() {
            std::fs::create_dir_all(&folder_path)?;
            info!("Created watch folder: {:?}", folder_path);
        }

        self.watcher.watch(&folder_path, RecursiveMode::NonRecursive)?;
        crate::add_log(&format!("Watching {:?} for {}", folder_path, tool_config.id));

        let mut folders = self.watched_folders.write().await;
        folders.insert(folder_path, tool_config);
        Ok(())
    }

    pub async fn remove_folder(&mut self, folder_path: &Path) -> Result<(), WatcherError> {
        self.watcher.unwatch(folder_path)?;
        let mut folders = self.watched_folders.write().await;
        folders.remove(folder_path);
        info!("Stopped watching folder: {:?}", folder_path);
        Ok(())
    }

    async fn process_events(
        rx: &mut mpsc::Receiver<Event>,
        watched_folders: Arc<RwLock<HashMap<PathBuf, ToolConfig>>>,
        event_sender: broadcast::Sender<FileEvent>,
    ) {
        let mut pending_files: HashMap<PathBuf, Instant> = HashMap::new();
        let debounce_duration = Duration::from_secs(2);

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    Self::handle_notify_event(event, &mut pending_files).await;
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    Self::check_pending_files(&mut pending_files, &watched_folders, &event_sender, debounce_duration).await;
                }
            }
        }
    }

    async fn handle_notify_event(event: Event, pending_files: &mut HashMap<PathBuf, Instant>) {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {}
            _ => return,
        }
        for path in event.paths {
            if !path.is_file() {
                continue;
            }
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if Self::is_in_processed_folder(&path) {
                continue;
            }
            if file_name.starts_with('.') || file_name.ends_with(".tmp") || file_name.ends_with(".part") || file_name.ends_with(".crdownload") {
                continue;
            }
            pending_files.insert(path, Instant::now());
        }
    }

    async fn check_pending_files(
        pending_files: &mut HashMap<PathBuf, Instant>,
        watched_folders: &Arc<RwLock<HashMap<PathBuf, ToolConfig>>>,
        event_sender: &broadcast::Sender<FileEvent>,
        debounce_duration: Duration,
    ) {
        let now = Instant::now();
        let ready_files: Vec<PathBuf> = pending_files
            .iter()
            .filter(|(path, last)| now.duration_since(**last) >= debounce_duration && path.exists() && Self::is_file_ready(path))
            .map(|(p, _)| p.clone())
            .collect();

        let folders = watched_folders.read().await;
        for path in ready_files {
            pending_files.remove(&path);
            let Some((_folder, tool_config)) = Self::find_watched_folder(&path, &folders) else { continue };

            // the folder decides the tool; the tool decides which files count
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !tool_config.accepts_extension(ext) {
                crate::add_log(&format!("Ignored {:?}: {} does not take .{}", path.file_name().unwrap_or_default(), tool_config.id, ext));
                continue;
            }
            // our own output landing in the same folder must not loop
            if Self::is_own_output(&path, tool_config) {
                continue;
            }

            info!("Processing file: {:?} with tool: {}", path, tool_config.id);
            let file_event = FileEvent { path: path.clone(), tool_id: tool_config.id.clone(), tool_config: tool_config.clone() };
            if let Err(e) = event_sender.send(file_event) {
                error!("Failed to send file event: {}", e);
            }
        }
    }

    fn is_own_output(path: &Path, config: &ToolConfig) -> bool {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        stem.ends_with(&format!("_{}", config.id)) || produced_outputs().lock().map(|set| set.contains(path)).unwrap_or(false)
    }

    fn is_in_processed_folder(path: &Path) -> bool {
        path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| s.eq_ignore_ascii_case("processed") || s.eq_ignore_ascii_case("originals"))
                .unwrap_or(false)
        })
    }

    fn is_file_ready(path: &Path) -> bool {
        std::fs::OpenOptions::new().read(true).open(path).is_ok()
    }

    fn find_watched_folder<'a>(
        file_path: &Path,
        watched_folders: &'a HashMap<PathBuf, ToolConfig>,
    ) -> Option<(&'a PathBuf, &'a ToolConfig)> {
        let mut best: Option<(&'a PathBuf, &'a ToolConfig)> = None;
        let mut best_len = 0;
        for (folder_path, config) in watched_folders {
            if file_path.starts_with(folder_path) {
                let len = folder_path.as_os_str().len();
                if len > best_len {
                    best_len = len;
                    best = Some((folder_path, config));
                }
            }
        }
        best
    }
}

/// Run one file through the tool of its folder: upload, wait, download,
/// then park the original in Originals/.
pub async fn process_file_event(event: FileEvent, auth_token: Option<String>) -> Result<PathBuf, crate::api::ApiError> {
    let output = run_tool(&event.path, &event.tool_config, auth_token, None).await?;
    if let Err(e) = move_to_originals(&event.path).await {
        info!("Could not move original file to Originals folder: {}", e);
    }
    Ok(output)
}

/// The shared job runner: watched folders and "open with"/dropped files
/// both end here. `output_dir` overrides the tool's output mode (used for
/// dropped files, which land next to their source).
/// Paths this process has written as results. A watcher on the same folder
/// sees them appear and must not feed them back in (names no longer carry the
/// tool id since the server decides the suffix, e.g. "Katalog_FOGRA39.pdf").
fn produced_outputs() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

pub async fn run_tool(
    input: &Path,
    tool: &ToolConfig,
    auth_token: Option<String>,
    output_dir: Option<&Path>,
) -> Result<PathBuf, crate::api::ApiError> {
    let client = PdfDkClient::new(auth_token);
    let endpoint_id = tool.endpoint.as_deref().map(|e| e.trim_start_matches("/api/")).unwrap_or(&tool.id).to_string();
    let file_field = tool.file_field.clone().unwrap_or_else(|| "file".to_string());

    let job_uuid = crate::api::with_retry("Upload", 3, || client.process_file(input, &endpoint_id, &file_field, &tool.options)).await?;
    let job = crate::api::with_retry("Status", 3, || client.poll_job(&job_uuid)).await?;

    // the server knows the real output type (zip, docx, svg …) and the suffix
    // it would give a web download ("komprimeret", or the detected colour
    // profile in check mode); the catalogue's `output` and the tool id are the
    // fallbacks. The original stem is kept as-is (the server's copy is ASCII-folded).
    let server_name = job.output_filename.as_deref().map(Path::new);
    let ext = server_name
        .and_then(|n| n.extension().and_then(|e| e.to_str()).map(|s| s.to_string()))
        .or_else(|| tool.output.clone())
        .unwrap_or_else(|| "pdf".to_string());
    let suffix = server_name
        .and_then(|n| n.file_stem().and_then(|s| s.to_str()))
        .and_then(|stem| stem.rsplit_once('_').map(|(_, sfx)| sfx.to_string()))
        .filter(|s| !s.is_empty() && s.len() <= 40)
        .unwrap_or_else(|| tool.id.clone());

    let output_path = get_output_path(input, tool, &suffix, &ext, output_dir);
    if let Ok(mut set) = produced_outputs().lock() {
        set.insert(output_path.clone());
    }
    crate::api::with_retry("Download", 3, || client.download_result(&job_uuid, &output_path)).await?;
    Ok(output_path)
}

/// Several files → one result (tools with a "files[]" field, e.g. images → one PDF).
/// The result lands next to the first file, named after it.
pub async fn run_tool_multi(
    inputs: &[PathBuf],
    tool: &ToolConfig,
    auth_token: Option<String>,
    output_dir: Option<&Path>,
) -> Result<PathBuf, crate::api::ApiError> {
    let first = inputs.first().ok_or_else(|| crate::api::ApiError::ServerError("no files".into()))?;
    let client = PdfDkClient::new(auth_token);
    let endpoint_id = tool.endpoint.as_deref().map(|e| e.trim_start_matches("/api/")).unwrap_or(&tool.id).to_string();
    let file_field = tool.file_field.clone().unwrap_or_else(|| "files[]".to_string());
    let job_uuid = crate::api::with_retry("Upload", 3, || client.process_files_multi(inputs, &endpoint_id, &file_field, &tool.options)).await?;
    let job = crate::api::with_retry("Status", 3, || client.poll_job(&job_uuid)).await?;
    let server_name = job.output_filename.as_deref().map(Path::new);
    let ext = server_name
        .and_then(|n| n.extension().and_then(|e| e.to_str()).map(|s| s.to_string()))
        .or_else(|| tool.output.clone())
        .unwrap_or_else(|| "pdf".to_string());
    let suffix = server_name
        .and_then(|n| n.file_stem().and_then(|s| s.to_str()))
        .and_then(|stem| stem.rsplit_once('_').map(|(_, sfx)| sfx.to_string()))
        .filter(|s| !s.is_empty() && s.len() <= 40)
        .unwrap_or_else(|| tool.id.clone());
    let output_path = get_output_path(first, tool, &suffix, &ext, output_dir);
    if let Ok(mut set) = produced_outputs().lock() {
        set.insert(output_path.clone());
    }
    crate::api::with_retry("Download", 3, || client.download_result(&job_uuid, &output_path)).await?;
    Ok(output_path)
}

async fn move_to_originals(file_path: &Path) -> Result<(), std::io::Error> {
    let parent = file_path.parent().unwrap_or(Path::new("."));
    let originals_folder = parent.join("Originals");
    tokio::fs::create_dir_all(&originals_folder).await?;
    let filename = file_path.file_name().unwrap_or_default();
    let dest_path = originals_folder.join(filename);
    let final_dest = if dest_path.exists() {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("pdf");
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        originals_folder.join(format!("{}_{}.{}", stem, ts, ext))
    } else {
        dest_path
    };
    tokio::fs::rename(file_path, &final_dest).await?;
    Ok(())
}

fn get_output_path(input_path: &Path, config: &ToolConfig, suffix: &str, ext: &str, output_dir: Option<&Path>) -> PathBuf {
    let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let parent = input_path.parent().unwrap_or(Path::new("."));
    let dir: PathBuf = match output_dir {
        Some(d) => d.to_path_buf(),
        None => match &config.output_mode {
            OutputMode::SameFolder => parent.to_path_buf(),
            OutputMode::Subfolder => parent.join("Processed"),
            OutputMode::Custom(custom) => PathBuf::from(custom),
        },
    };
    unique_path(&dir, &format!("{}_{}", stem, suffix), ext)
}

/// "Katalog_komprimeret.pdf", then "Katalog_komprimeret (2).pdf", … — a second run
/// must never overwrite the first result silently.
fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{}.{}", base, ext));
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let p = dir.join(format!("{} ({}).{}", base, n, ext));
        if !p.exists() {
            return p;
        }
    }
    first
}
