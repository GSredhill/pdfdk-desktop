// Job processor module for PDF.dk Desktop
// Manages the job queue and processing state

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub tool_id: String,
    pub input_file: String,
    pub output_file: Option<String>,
    pub status: JobStatus,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Uploading,
    Processing,
    Downloading,
    Completed,
    Failed,
}

impl Job {
    pub fn new(tool_id: &str, input_file: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool_id: tool_id.to_string(),
            input_file: input_file.to_string(),
            output_file: None,
            status: JobStatus::Pending,
            progress: None,
            error: None,
            created_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            completed_at: None,
        }
    }

    pub fn set_uploading(&mut self) {
        self.status = JobStatus::Uploading;
        self.progress = Some(10);
    }

    pub fn set_processing(&mut self) {
        self.status = JobStatus::Processing;
        self.progress = Some(50);
    }

    pub fn set_downloading(&mut self) {
        self.status = JobStatus::Downloading;
        self.progress = Some(80);
    }

    pub fn set_completed(&mut self, output_file: &str) {
        self.status = JobStatus::Completed;
        self.progress = Some(100);
        self.output_file = Some(output_file.to_string());
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    pub fn set_failed(&mut self, error: &str) {
        self.status = JobStatus::Failed;
        self.error = Some(error.to_string());
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }
}
