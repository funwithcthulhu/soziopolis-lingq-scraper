use crate::{
    app_paths, credential_store,
    database::Database,
    database::{LibraryStats, StoredArticle},
    jobs::{
        CompletedJob, FailedFetchItem, ImportProgress, JobKind, QueueSnapshot, QueuedJob,
        QueuedJobRequest, UploadFailure, UploadProgress,
    },
    lingq::Collection,
    logging,
    repositories::{ArticleRepository, JobRepository},
    services::{
        BrowseResponse, BrowseService, BrowseSessionState, ContentRefreshResult, LibraryService,
        LingqService,
    },
    settings::SettingsStore,
    soziopolis::{Article, ArticleSummary, DiscoveryReport, SECTIONS},
    topics::generated_topic_from_fields,
};
use chrono::NaiveDate;
use eframe::egui::{
    self, Align, Color32, Context, Frame, Layout, Margin, ProgressBar, RichText, ScrollArea,
    SidePanel, Stroke, TextEdit, TopBottomPanel, ViewportBuilder,
};
use std::{
    any::Any,
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod actions;
mod diagnostics;
mod events;
mod helpers;
mod jobs;
mod shell;
mod views;

use helpers::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Browse,
    Library,
    Article,
    Diagnostics,
}

impl View {
    fn as_str(self) -> &'static str {
        match self {
            Self::Browse => "browse",
            Self::Library => "library",
            Self::Article => "article",
            Self::Diagnostics => "diagnostics",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "library" => Self::Library,
            "lingq" => Self::Library,
            "article" => Self::Article,
            "diagnostics" => Self::Diagnostics,
            _ => Self::Browse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowseScope {
    CurrentSection,
    AllSections,
}

impl BrowseScope {
    fn label(self) -> &'static str {
        match self {
            Self::CurrentSection => "Current section",
            Self::AllSections => "All sections",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NoticeKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LingqAuthMode {
    Account,
    Token,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibrarySortMode {
    Newest,
    Oldest,
    Longest,
    Shortest,
    Title,
}

impl LibrarySortMode {
    fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::Longest => "Longest",
            Self::Shortest => "Shortest",
            Self::Title => "Title",
        }
    }
}

struct Notice {
    message: String,
    kind: NoticeKind,
    created_at: Instant,
}

struct ActiveJob {
    id: u64,
    kind: JobKind,
    label: String,
    total: usize,
    processed: usize,
    succeeded: usize,
    failed: usize,
    current_item: String,
    cancel_flag: Arc<AtomicBool>,
}

enum AppEvent {
    BrowseLoaded {
        request_id: u64,
        result: Result<BrowseResponse, String>,
    },
    PreviewLoaded(Result<Article, String>),
    BatchFetchProgress(ImportProgress),
    BatchFetched {
        job_id: u64,
        saved_count: usize,
        skipped_existing: usize,
        skipped_out_of_range: usize,
        failed: Vec<FailedFetchItem>,
        canceled: bool,
    },
    ContentRefreshCompleted {
        request_id: u64,
        reason: String,
        result: ContentRefreshResult,
    },
    LingqLoggedIn(Result<String, String>),
    CollectionsLoaded(Result<Vec<Collection>, String>),
    UploadProgress {
        job_id: u64,
        progress: UploadProgress,
    },
    BatchUploaded {
        job_id: u64,
        uploaded: usize,
        failed: Vec<UploadFailure>,
        canceled: bool,
    },
}

pub fn run() -> eframe::Result<()> {
    if let Ok(log_path) = logging::init() {
        logging::info(format!(
            "GUI run requested; log path {}",
            log_path.display()
        ));
    }
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([1024.0, 720.0])
            .with_maximized(true)
            .with_title("Soziopolis Reader"),
        ..Default::default()
    };

    eframe::run_native(
        "Soziopolis Reader",
        options,
        Box::new(|cc| Ok(Box::new(SoziopolisLingqGui::new(cc)))),
    )
}

pub struct SoziopolisLingqGui {
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    settings: SettingsStore,
    current_view: View,
    notice: Option<Notice>,
    browse_request_id: u64,
    content_refresh_request_id: u64,

    browse_section: String,
    browse_limit: usize,
    browse_scope: BrowseScope,
    browse_articles: Vec<ArticleSummary>,
    browse_report: Option<DiscoveryReport>,
    browse_scope_label: String,
    browse_search: String,
    browse_selected: HashSet<String>,
    browse_only_new: bool,
    browse_imported_urls: HashSet<String>,
    browse_loading: bool,
    browse_end_reached: bool,
    browse_session_state: Option<BrowseSessionState>,
    batch_fetching: bool,
    failed_fetches: Vec<FailedFetchItem>,
    import_progress: Option<ImportProgress>,
    upload_progress: Option<UploadProgress>,
    preview_article: Option<Article>,
    preview_stored_article: Option<StoredArticle>,
    preview_loading: bool,
    show_preview: bool,

    library_articles: Vec<StoredArticle>,
    library_stats: Option<LibraryStats>,
    library_loading: bool,
    library_search: String,
    library_topic: String,
    library_only_not_uploaded: bool,
    library_word_count_min: String,
    library_word_count_max: String,
    library_group_by_topic: bool,
    library_sort_mode: LibrarySortMode,
    library_filters_expanded: bool,
    article_detail: Option<StoredArticle>,

    lingq_api_key: String,
    lingq_auth_mode: LingqAuthMode,
    lingq_username: String,
    lingq_password: String,
    lingq_connected: bool,
    lingq_collections: Vec<Collection>,
    lingq_selected_collection: Option<i64>,
    lingq_selected_articles: HashSet<i64>,
    lingq_word_count_min: String,
    lingq_word_count_max: String,
    lingq_select_only_not_uploaded: bool,
    show_lingq_settings: bool,
    lingq_loading_collections: bool,
    lingq_uploading: bool,

    next_job_id: u64,
    queue_paused: bool,
    active_job: Option<ActiveJob>,
    queued_jobs: VecDeque<QueuedJob>,
    completed_jobs: VecDeque<CompletedJob>,
    last_failed_uploads: Vec<UploadFailure>,
}
