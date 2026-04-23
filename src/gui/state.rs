use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Browse,
    Library,
    Article,
    Diagnostics,
}

impl View {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Browse => "browse",
            Self::Library => "library",
            Self::Article => "article",
            Self::Diagnostics => "diagnostics",
        }
    }

    pub(super) fn from_str(value: &str) -> Self {
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
pub(super) enum BrowseScope {
    CurrentSection,
    AllSections,
}

impl BrowseScope {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::CurrentSection => "Current section",
            Self::AllSections => "All sections",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum NoticeKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LingqAuthMode {
    Account,
    Token,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibrarySortMode {
    Newest,
    Oldest,
    Longest,
    Shortest,
    Title,
}

impl LibrarySortMode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::Longest => "Longest",
            Self::Shortest => "Shortest",
            Self::Title => "Title",
        }
    }
}

pub(super) struct Notice {
    pub(super) message: String,
    pub(super) kind: NoticeKind,
    pub(super) created_at: Instant,
}

pub(super) struct ActiveJob {
    pub(super) id: u64,
    pub(super) kind: JobKind,
    pub(super) label: String,
    pub(super) total: usize,
    pub(super) processed: usize,
    pub(super) succeeded: usize,
    pub(super) failed: usize,
    pub(super) current_item: String,
    pub(super) cancel_flag: Arc<AtomicBool>,
}

pub(super) enum AppEvent {
    BrowseLoaded {
        request_id: u64,
        result: Result<BrowseResponse, String>,
    },
    PreviewLoaded(Result<Article, String>),
    BatchFetchProgress(ImportProgress),
    BatchFetched {
        job_id: u64,
        saved_count: usize,
        saved_articles: Vec<StoredArticle>,
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
        successes: Vec<UploadSuccess>,
        failed: Vec<UploadFailure>,
        canceled: bool,
    },
}

pub struct SoziopolisLingqGui {
    pub(super) tx: Sender<AppEvent>,
    pub(super) rx: Receiver<AppEvent>,
    pub(super) settings: SettingsStore,
    pub(super) current_view: View,
    pub(super) notice: Option<Notice>,
    pub(super) browse_request_id: u64,
    pub(super) content_refresh_request_id: u64,

    pub(super) browse_section: String,
    pub(super) browse_limit: usize,
    pub(super) browse_scope: BrowseScope,
    pub(super) browse_articles: Vec<ArticleSummary>,
    pub(super) browse_report: Option<DiscoveryReport>,
    pub(super) browse_scope_label: String,
    pub(super) browse_search: String,
    pub(super) browse_selected: HashSet<String>,
    pub(super) browse_only_new: bool,
    pub(super) browse_imported_urls: HashSet<String>,
    pub(super) browse_loading: bool,
    pub(super) browse_end_reached: bool,
    pub(super) browse_session_state: Option<BrowseSessionState>,
    pub(super) batch_fetching: bool,
    pub(super) failed_fetches: Vec<FailedFetchItem>,
    pub(super) import_progress: Option<ImportProgress>,
    pub(super) upload_progress: Option<UploadProgress>,
    pub(super) preview_article: Option<Article>,
    pub(super) preview_stored_article: Option<StoredArticle>,
    pub(super) preview_loading: bool,
    pub(super) show_preview: bool,

    pub(super) library_articles: Vec<StoredArticle>,
    pub(super) library_stats: Option<LibraryStats>,
    pub(super) library_loading: bool,
    pub(super) library_search: String,
    pub(super) library_topic: String,
    pub(super) library_only_not_uploaded: bool,
    pub(super) library_word_count_min: String,
    pub(super) library_word_count_max: String,
    pub(super) library_group_by_topic: bool,
    pub(super) library_sort_mode: LibrarySortMode,
    pub(super) library_filters_expanded: bool,
    pub(super) library_search_cache_query: String,
    pub(super) library_search_cache_results: Vec<StoredArticle>,
    pub(super) article_detail: Option<StoredArticle>,

    pub(super) lingq_api_key: String,
    pub(super) lingq_auth_mode: LingqAuthMode,
    pub(super) lingq_username: String,
    pub(super) lingq_password: String,
    pub(super) lingq_connected: bool,
    pub(super) lingq_collections: Vec<Collection>,
    pub(super) lingq_selected_collection: Option<i64>,
    pub(super) lingq_selected_articles: HashSet<i64>,
    pub(super) lingq_word_count_min: String,
    pub(super) lingq_word_count_max: String,
    pub(super) lingq_select_only_not_uploaded: bool,
    pub(super) show_lingq_settings: bool,
    pub(super) lingq_loading_collections: bool,
    pub(super) lingq_uploading: bool,

    pub(super) next_job_id: u64,
    pub(super) queue_paused: bool,
    pub(super) active_job: Option<ActiveJob>,
    pub(super) queued_jobs: VecDeque<QueuedJob>,
    pub(super) completed_jobs: VecDeque<CompletedJob>,
    pub(super) last_failed_uploads: Vec<UploadFailure>,
    pub(super) diagnostics_selected_job_id: Option<u64>,
}
