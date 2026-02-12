// File watcher module for PDF.dk Desktop
// Watches folders for new PDF files and triggers processing

use crate::api::PdfDkClient;
use crate::config::{OutputMode, ToolConfig};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, info, warn};

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

/// Folder watcher that monitors multiple folders for new PDF files
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
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Log every event we receive
                        crate::add_log(&format!("File system event: {:?}", event.kind));
                        // Use blocking_send since we're in a sync callback
                        if let Err(e) = notify_tx.blocking_send(event) {
                            crate::add_log(&format!("Failed to send event to channel: {}", e));
                        }
                    }
                    Err(e) => {
                        crate::add_log(&format!("File watcher error: {}", e));
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        let watched_folders = Arc::new(RwLock::new(HashMap::new()));

        let folder_watcher = Self {
            watcher,
            watched_folders: watched_folders.clone(),
            event_sender: event_tx.clone(),
        };

        // Spawn event processor with shared watched_folders
        let event_sender = event_tx;
        let wf = watched_folders.clone();

        tokio::spawn(async move {
            Self::process_events(&mut notify_rx, wf, event_sender).await;
        });

        Ok((folder_watcher, event_rx))
    }

    /// Add a folder to watch
    pub async fn add_folder(&mut self, tool_config: ToolConfig) -> Result<(), WatcherError> {
        let folder_path = match &tool_config.folder_path {
            Some(path) => PathBuf::from(path),
            None => return Ok(()), // No folder configured
        };

        if !tool_config.enabled {
            return Ok(()); // Tool disabled
        }

        // Create folder if it doesn't exist
        if !folder_path.exists() {
            std::fs::create_dir_all(&folder_path)?;
            info!("Created watch folder: {:?}", folder_path);
        }

        // Start watching
        crate::add_log(&format!("Starting watch on folder: {:?}", folder_path));
        self.watcher
            .watch(&folder_path, RecursiveMode::NonRecursive)?;
        crate::add_log(&format!("Successfully watching: {:?} for tool: {}", folder_path, tool_config.id));

        // Add to shared watched_folders
        {
            let mut folders = self.watched_folders.write().await;
            folders.insert(folder_path.clone(), tool_config.clone());
            crate::add_log(&format!("Registered {} watched folders:", folders.len()));
            for (path, config) in folders.iter() {
                crate::add_log(&format!("  - {} -> {:?}", config.id, path));
            }
        }

        Ok(())
    }

    /// Remove a folder from watching
    pub async fn remove_folder(&mut self, folder_path: &Path) -> Result<(), WatcherError> {
        self.watcher.unwatch(folder_path)?;
        {
            let mut folders = self.watched_folders.write().await;
            folders.remove(folder_path);
        }
        info!("Stopped watching folder: {:?}", folder_path);
        Ok(())
    }

    /// Process notify events and emit file events
    async fn process_events(
        rx: &mut mpsc::Receiver<Event>,
        watched_folders: Arc<RwLock<HashMap<PathBuf, ToolConfig>>>,
        event_sender: broadcast::Sender<FileEvent>,
    ) {
        crate::add_log("File watcher event processor started - listening for file changes...");
        let mut pending_files: HashMap<PathBuf, Instant> = HashMap::new();
        let debounce_duration = Duration::from_secs(2);

        loop {
            // Use tokio::select to either receive an event or timeout
            tokio::select! {
                Some(event) = rx.recv() => {
                    info!("Got event from notify channel: {:?}", event);
                    Self::handle_notify_event(
                        event,
                        &mut pending_files,
                    )
                    .await;
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    // Check for files that have stabilized
                    Self::check_pending_files(
                        &mut pending_files,
                        &watched_folders,
                        &event_sender,
                        debounce_duration,
                    )
                    .await;
                }
            }
        }
    }

    async fn handle_notify_event(
        event: Event,
        pending_files: &mut HashMap<PathBuf, Instant>,
    ) {
        crate::add_log(&format!("Processing event: {:?}", event.kind));

        // Only handle create and modify events
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {}
            _ => {
                crate::add_log(&format!("Skipping event type: {:?}", event.kind));
                return;
            }
        }

        for path in event.paths {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
            crate::add_log(&format!("Checking file: {}", file_name));

            // Skip if not a PDF file
            if !Self::is_pdf_file(&path) {
                crate::add_log(&format!("Skipping non-PDF: {}", file_name));
                continue;
            }

            // Skip if in a "Processed" subfolder
            if Self::is_in_processed_folder(&path) {
                crate::add_log(&format!("Skipping file in Processed/Originals folder: {}", file_name));
                continue;
            }

            // Skip temporary/partial files
            if file_name.starts_with('.') || file_name.ends_with(".tmp") || file_name.ends_with(".part") {
                crate::add_log(&format!("Skipping temp file: {}", file_name));
                continue;
            }

            crate::add_log(&format!("PDF detected, adding to queue: {}", file_name));

            // Add to pending files for debouncing
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
        let mut ready_files = Vec::new();

        // Find files that have stabilized
        for (path, last_event) in pending_files.iter() {
            if now.duration_since(*last_event) >= debounce_duration {
                // Check if file still exists and is readable
                if path.exists() && Self::is_file_ready(path) {
                    ready_files.push(path.clone());
                }
            }
        }

        // Process ready files
        let folders = watched_folders.read().await;
        for path in ready_files {
            pending_files.remove(&path);

            // Find which watched folder this file belongs to
            if let Some((_folder_path, tool_config)) = Self::find_watched_folder(&path, &folders) {
                info!("Processing file: {:?} with tool: {}", path, tool_config.id);

                let file_event = FileEvent {
                    path: path.clone(),
                    tool_id: tool_config.id.clone(),
                    tool_config: tool_config.clone(),
                };

                if let Err(e) = event_sender.send(file_event) {
                    error!("Failed to send file event: {}", e);
                }
            }
        }
    }

    fn is_pdf_file(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
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
        // Try to open the file for reading to check if it's accessible and not being written
        match std::fs::OpenOptions::new().read(true).open(path) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    fn find_watched_folder<'a>(
        file_path: &Path,
        watched_folders: &'a HashMap<PathBuf, ToolConfig>,
    ) -> Option<(&'a PathBuf, &'a ToolConfig)> {
        // Find the most specific (longest) matching folder path
        // This is important because HashMap iteration order is not guaranteed
        let mut best_match: Option<(&'a PathBuf, &'a ToolConfig)> = None;
        let mut best_len = 0;

        for (folder_path, config) in watched_folders {
            if file_path.starts_with(folder_path) {
                let path_len = folder_path.as_os_str().len();
                if path_len > best_len {
                    best_len = path_len;
                    best_match = Some((folder_path, config));
                }
            }
        }
        best_match
    }
}

/// Process a file event using the PDF.dk API
pub async fn process_file_event(
    event: FileEvent,
    auth_token: Option<String>,
) -> Result<PathBuf, crate::api::ApiError> {
    let client = PdfDkClient::new(auth_token);

    // Determine output path
    let output_path = get_output_path(&event.path, &event.tool_config);

    // Get tool options
    let options = event.tool_config.options.clone();

    // Process the file
    client
        .process_and_download(&event.path, &output_path, &event.tool_id, options)
        .await?;

    // Move original file to Originals folder after successful processing
    if let Err(e) = move_to_originals(&event.path).await {
        // Log warning but don't fail - the processing was successful
        info!("Could not move original file to Originals folder: {}", e);
    }

    Ok(output_path)
}

/// Move the original file to an "Originals" subfolder
async fn move_to_originals(file_path: &Path) -> Result<(), std::io::Error> {
    let parent = file_path.parent().unwrap_or(Path::new("."));
    let originals_folder = parent.join("Originals");

    // Create Originals folder if it doesn't exist
    tokio::fs::create_dir_all(&originals_folder).await?;

    // Get filename
    let filename = file_path.file_name().unwrap_or_default();
    let dest_path = originals_folder.join(filename);

    // If file already exists in Originals, add timestamp to avoid overwrite
    let final_dest = if dest_path.exists() {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("pdf");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        originals_folder.join(format!("{}_{}.{}", stem, timestamp, ext))
    } else {
        dest_path
    };

    // Move the file
    tokio::fs::rename(file_path, &final_dest).await?;
    info!("Moved original file to: {:?}", final_dest);

    Ok(())
}

/// Get the output path for a processed file
fn get_output_path(input_path: &Path, config: &ToolConfig) -> PathBuf {
    let file_stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    // Determine output extension based on tool type
    let extension = match config.id.as_str() {
        "pdf-to-word" => "docx",
        "pdf-to-excel" => "xlsx",
        "pdf-to-jpg" => "zip",  // Returns zip of images
        _ => "pdf",  // All other tools output PDF
    };

    let output_filename = format!("{}_{}.{}", file_stem, config.id, extension);

    match &config.output_mode {
        OutputMode::SameFolder => {
            input_path.parent().unwrap_or(Path::new(".")).join(&output_filename)
        }
        OutputMode::Subfolder => {
            let parent = input_path.parent().unwrap_or(Path::new("."));
            parent.join("Processed").join(&output_filename)
        }
        OutputMode::Custom(custom_path) => {
            PathBuf::from(custom_path).join(&output_filename)
        }
    }
}
