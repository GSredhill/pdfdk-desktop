// API client for PDF.dk
// Handles file upload, job polling, and download

use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tokio::fs;
use tracing::{debug, info};
use uuid::Uuid;

const API_BASE_URL: &str = "https://pdf.dk/api";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_POLL_ATTEMPTS: u32 = 300; // 10 minutes max

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Job failed: {0}")]
    JobFailed(String),
    #[error("Job timeout")]
    Timeout,
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Unauthorized - please login again")]
    Unauthorized,
    #[error("Monthly job limit exceeded")]
    JobLimitExceeded,
    #[error("File too large for your plan (max {0} MB)")]
    FileTooLarge(i32),
}

// Response from upload endpoints (compress, pdf-to-word, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<UploadData>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadData {
    pub job_uuid: String,
    pub status: String,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// Usage status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStatusResponse {
    pub success: bool,
    pub data: Option<UsageStatusData>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStatusData {
    pub plan: String,
    pub limit: i32,
    pub used: i32,
    #[serde(default)]
    pub is_unlimited: bool,
    #[serde(default)]
    pub is_authenticated: bool,
    #[serde(default)]
    pub batch_upload: bool,
    #[serde(default)]
    pub max_file_size_mb: Option<i32>,
}

// Response from job status polling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusResponse {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<JobStatusData>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusData {
    pub uuid: String,
    pub status: String,
    pub progress: Option<u8>,
    pub output_path: Option<String>,
    pub output_filename: Option<String>,
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Unknown(String),
}

impl From<&str> for JobStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "queued" => JobStatus::Queued,
            "processing" => JobStatus::Processing,
            "completed" | "done" => JobStatus::Completed,
            "failed" | "error" => JobStatus::Failed,
            other => JobStatus::Unknown(other.to_string()),
        }
    }
}

/// PDF.dk API Client
pub struct PdfDkClient {
    client: Client,
    auth_token: Option<String>,
    session_id: String,
}

impl PdfDkClient {
    pub fn new(auth_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        // Generate a session ID for this client instance
        let session_id = Uuid::new_v4().to_string();

        Self { client, auth_token, session_id }
    }

    /// Process a PDF file with the specified tool
    /// Returns the job UUID for polling
    pub async fn process_file(
        &self,
        file_path: &Path,
        tool: &str,
        options: serde_json::Value,
    ) -> Result<String, ApiError> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file.pdf")
            .to_string();

        info!("Uploading file: {} for tool: {}", file_name, tool);

        let file_bytes = fs::read(file_path).await?;

        let mut form = multipart::Form::new().part(
            "file",
            multipart::Part::bytes(file_bytes)
                .file_name(file_name.clone())
                .mime_str("application/pdf")
                .unwrap(),
        );

        // Add options as form fields
        if let Some(obj) = options.as_object() {
            for (key, value) in obj {
                if let Some(s) = value.as_str() {
                    form = form.text(key.clone(), s.to_string());
                } else {
                    form = form.text(key.clone(), value.to_string());
                }
            }
        }

        let url = format!("{}/{}", API_BASE_URL, tool);
        debug!("POST {}", url);

        let mut request = self.client.post(&url)
            .multipart(form)
            .header("X-Session-ID", &self.session_id)
            .header("Accept", "application/json");

        // Add auth header if we have a token
        if let Some(ref token) = self.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }

        // Handle rate limiting (429) - job limit exceeded
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiError::JobLimitExceeded);
        }

        // Handle file too large (413)
        if status == reqwest::StatusCode::PAYLOAD_TOO_LARGE {
            // Try to parse the response to get the max file size
            let body = response.text().await.unwrap_or_default();
            // Default to 100MB if we can't parse
            return Err(ApiError::FileTooLarge(100));
        }

        let body = response.text().await.unwrap_or_default();

        info!("API Response status: {}", status);
        info!("API Response body: {}", body);

        if !status.is_success() {
            return Err(ApiError::ServerError(format!(
                "Server returned {}: {}",
                status, body
            )));
        }

        let upload_response: UploadResponse = serde_json::from_str(&body)
            .map_err(|e| ApiError::ServerError(format!("Failed to parse response: {} - Body: {}", e, body)))?;

        if !upload_response.success {
            return Err(ApiError::ServerError(
                upload_response
                    .error
                    .or(upload_response.message)
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        upload_response.data
            .map(|d| d.job_uuid)
            .ok_or(ApiError::ServerError("No job UUID returned from server".to_string()))
    }

    /// Poll job status until completion
    pub async fn poll_job(&self, uuid: &str) -> Result<JobStatusData, ApiError> {
        let url = format!("{}/jobs/{}", API_BASE_URL, uuid);
        let mut attempts = 0;

        loop {
            attempts += 1;
            if attempts > MAX_POLL_ATTEMPTS {
                return Err(ApiError::Timeout);
            }

            debug!("Polling job {} (attempt {})", uuid, attempts);

            let mut request = self.client.get(&url)
                .header("X-Session-ID", &self.session_id)
                .header("Accept", "application/json");

            if let Some(ref token) = self.auth_token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request.send().await?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }

            let body = response.text().await.unwrap_or_default();
            debug!("Poll response: {}", body);

            let job_response: JobStatusResponse = serde_json::from_str(&body)
                .map_err(|e| ApiError::ServerError(format!("Failed to parse poll response: {} - Body: {}", e, body)))?;

            if !job_response.success {
                // Check if it's an auth error or actual job error
                let msg = job_response.error.or(job_response.message).unwrap_or_default();
                if msg.to_lowercase().contains("unauthorized") {
                    return Err(ApiError::Unauthorized);
                }
                return Err(ApiError::ServerError(msg));
            }

            if let Some(job) = job_response.data {
                let status = JobStatus::from(job.status.as_str());
                match status {
                    JobStatus::Completed => {
                        info!("Job {} completed", uuid);
                        return Ok(job);
                    }
                    JobStatus::Failed => {
                        return Err(ApiError::JobFailed(
                            job.error.unwrap_or_else(|| "Unknown error".to_string()),
                        ));
                    }
                    _ => {
                        // Still processing, wait and retry
                        info!("Job {} status: {:?}, waiting...", uuid, status);
                        tokio::time::sleep(POLL_INTERVAL).await;
                    }
                }
            } else {
                // No job info, wait and retry
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    /// Download the completed file
    pub async fn download_result(&self, uuid: &str, output_path: &Path) -> Result<(), ApiError> {
        let url = format!("{}/jobs/{}/download", API_BASE_URL, uuid);

        info!("Downloading result to: {:?}", output_path);

        let mut request = self.client.get(&url)
            .header("X-Session-ID", &self.session_id)
            .header("Accept", "application/octet-stream");

        if let Some(ref token) = self.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::ServerError(format!(
                "Download failed: {}",
                body
            )));
        }

        let bytes = response.bytes().await?;

        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(output_path, bytes).await?;

        info!("Downloaded {} bytes to {:?}", output_path.metadata()?.len(), output_path);

        Ok(())
    }

    /// Get usage status for the current user
    pub async fn get_usage_status(&self) -> Result<UsageStatusData, ApiError> {
        let url = format!("{}/settings/usage-status", API_BASE_URL);

        let mut request = self.client.get(&url)
            .header("X-Session-ID", &self.session_id)
            .header("Accept", "application/json");

        if let Some(ref token) = self.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }

        let body = response.text().await.unwrap_or_default();
        debug!("Usage status response: {}", body);

        let usage_response: UsageStatusResponse = serde_json::from_str(&body)
            .map_err(|e| ApiError::ServerError(format!("Failed to parse usage response: {} - Body: {}", e, body)))?;

        if !usage_response.success {
            return Err(ApiError::ServerError(
                usage_response.message.unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        usage_response.data
            .ok_or(ApiError::ServerError("No usage data returned".to_string()))
    }

    /// Full process: upload, poll, download
    pub async fn process_and_download(
        &self,
        input_path: &Path,
        output_path: &Path,
        tool: &str,
        options: serde_json::Value,
    ) -> Result<(), ApiError> {
        // Upload and start processing
        let job_uuid = self.process_file(input_path, tool, options).await?;

        // Poll until complete
        let _completed_job = self.poll_job(&job_uuid).await?;

        // Download result
        self.download_result(&job_uuid, output_path).await?;

        Ok(())
    }
}
