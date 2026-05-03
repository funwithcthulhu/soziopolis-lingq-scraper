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
            "library" | "lingq" => Self::Library,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl std::fmt::Display for LingqAuthMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Account => write!(f, "Account Login"),
            Self::Token => write!(f, "Token / API Key"),
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

pub struct App {
    pub(super) app_context: Option<AppContext>,
    pub(super) app_context_error: Option<String>,
    pub(super) settings: SettingsStore,
    pub(super) current_view: View,
    pub(super) notice: Option<Notice>,
    pub(super) browse_request_id: u64,
    pub(super) content_refresh_request_id: u64,

    // Browse state
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

    // Library state
    pub(super) library_articles: Vec<ArticleListItem>,
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
    pub(super) library_dense_mode: bool,
    pub(super) library_page_index: usize,
    pub(super) library_page_cache: Option<ArticleListPage>,
    pub(super) article_detail: Option<StoredArticle>,

    // LingQ state
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

    // Job queue
    pub(super) next_job_id: u64,
    pub(super) queue_paused: bool,
    pub(super) active_job: Option<ActiveJob>,
    pub(super) queued_jobs: VecDeque<QueuedJob>,
    pub(super) completed_jobs: VecDeque<CompletedJob>,
    pub(super) last_failed_uploads: Vec<UploadFailure>,
    pub(super) recent_task_failures: VecDeque<AppError>,
    pub(super) diagnostics_selected_job_id: Option<u64>,
}

impl App {
    pub(super) fn new() -> (Self, Task<Message>) {
        let (settings, settings_notice) = match SettingsStore::load_default() {
            Ok(s) => (s, None),
            Err(err) => {
                logging::warn(format!("could not load settings: {err}"));
                let fallback = SettingsStore::create_default().unwrap_or_else(|_| {
                    SettingsStore::from_parts(
                        std::env::current_dir()
                            .unwrap_or_default()
                            .join("soziopolis_reader_settings.json"),
                        crate::settings::AppSettings::default(),
                    )
                });
                (
                    fallback,
                    Some(format!("Started with default settings: {err}")),
                )
            }
        };

        let current_view = View::from_str(&settings.data().last_view);
        let browse_section = settings.data().browse_section.clone();
        let browse_only_new = settings.data().browse_only_new;
        let lingq_selected_collection = settings.data().lingq_collection_id;

        let (lingq_api_key, startup_notice) = load_lingq_api_key_from_storage();
        let lingq_connected = !lingq_api_key.trim().is_empty();

        let (app_context, app_context_error) = match AppContext::shared() {
            Ok(ctx) => (Some(ctx), None),
            Err(err) => (None, Some(err.to_string())),
        };

        let mut app = Self {
            app_context,
            app_context_error: app_context_error.clone(),
            settings,
            current_view,
            notice: None,
            browse_request_id: 0,
            content_refresh_request_id: 0,
            browse_section: browse_section.clone(),
            browse_limit: 80,
            browse_scope: BrowseScope::CurrentSection,
            browse_articles: Vec::new(),
            browse_report: None,
            browse_scope_label: "Current section".to_owned(),
            browse_search: String::new(),
            browse_selected: HashSet::new(),
            browse_only_new,
            browse_imported_urls: HashSet::new(),
            browse_loading: false,
            browse_end_reached: false,
            browse_session_state: None,
            batch_fetching: false,
            failed_fetches: Vec::new(),
            import_progress: None,
            upload_progress: None,
            preview_article: None,
            preview_stored_article: None,
            preview_loading: false,
            show_preview: false,
            library_articles: Vec::new(),
            library_stats: None,
            library_loading: false,
            library_search: String::new(),
            library_topic: String::new(),
            library_only_not_uploaded: false,
            library_word_count_min: String::new(),
            library_word_count_max: String::new(),
            library_group_by_topic: true,
            library_sort_mode: LibrarySortMode::Newest,
            library_filters_expanded: true,
            library_dense_mode: false,
            library_page_index: 0,
            library_page_cache: None,
            article_detail: None,
            lingq_api_key,
            lingq_auth_mode: LingqAuthMode::Account,
            lingq_username: String::new(),
            lingq_password: String::new(),
            lingq_connected,
            lingq_collections: Vec::new(),
            lingq_selected_collection,
            lingq_selected_articles: HashSet::new(),
            lingq_word_count_min: String::new(),
            lingq_word_count_max: String::new(),
            lingq_select_only_not_uploaded: true,
            show_lingq_settings: false,
            lingq_loading_collections: false,
            lingq_uploading: false,
            next_job_id: 0,
            queue_paused: false,
            active_job: None,
            queued_jobs: VecDeque::new(),
            completed_jobs: VecDeque::new(),
            last_failed_uploads: Vec::new(),
            recent_task_failures: VecDeque::new(),
            diagnostics_selected_job_id: None,
        };

        app.load_persisted_queue_state();

        if let Some(msg) = settings_notice.or(startup_notice).or(app_context_error) {
            app.set_notice(msg, NoticeKind::Info);
        }

        // Build initial tasks: browse + content refresh + optional collections
        let browse_task = app.spawn_browse_refresh();
        let refresh_task = app.spawn_content_refresh("app startup");
        let collections_task = if app.lingq_connected {
            app.spawn_load_collections()
        } else {
            Task::none()
        };
        let queue_task = app.try_start_next_queued_job();

        let init_task = Task::batch([browse_task, refresh_task, collections_task, queue_task]);
        (app, init_task)
    }

    pub(super) fn theme(&self) -> Theme {
        Theme::custom("Soziopolis Dark".to_owned(), soziopolis_palette())
    }

    pub(super) fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }
}
