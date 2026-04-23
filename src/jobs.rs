use crate::soziopolis::ArticleSummary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedFetchItem {
    pub url: String,
    pub title: String,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ImportProgress {
    pub phase: String,
    pub processed: usize,
    pub total: Option<usize>,
    pub saved_count: usize,
    pub skipped_existing: usize,
    pub skipped_out_of_range: usize,
    pub failed_count: usize,
    pub current_item: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Import,
    Upload,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Import => "Import",
            Self::Upload => "LingQ upload",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadFailure {
    pub article_id: i64,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct UploadProgress {
    pub processed: usize,
    pub total: usize,
    pub uploaded: usize,
    pub failed_count: usize,
    pub current_item: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuedJobRequest {
    Import {
        articles: Vec<ArticleSummary>,
    },
    Upload {
        ids: Vec<i64>,
        collection_id: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedJob {
    pub id: u64,
    pub kind: JobKind,
    pub label: String,
    pub total: usize,
    pub request: QueuedJobRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedJob {
    pub id: u64,
    pub kind: JobKind,
    pub label: String,
    pub summary: String,
    pub success: bool,
    #[serde(default)]
    pub recorded_at: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub next_job_id: u64,
    pub queue_paused: bool,
    pub queued_jobs: Vec<QueuedJob>,
    pub completed_jobs: Vec<CompletedJob>,
    pub failed_fetches: Vec<FailedFetchItem>,
    pub failed_uploads: Vec<UploadFailure>,
}
