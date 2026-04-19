use crate::{
    app_paths, credential_store,
    database::{Database, LibraryStats, StoredArticle},
    lingq::{Collection, LingqClient, UploadRequest},
    logging,
    settings::SettingsStore,
    soziopolis::{Article, ArticleSummary, BrowseSectionResult, DiscoveryReport, SoziopolisClient},
    topics::generated_topic_from_fields,
};
use chrono::NaiveDate;
use eframe::egui::{
    self, Align, Color32, Context, Frame, Layout, Margin, ProgressBar, RichText, ScrollArea,
    SidePanel, Stroke, TextEdit, TopBottomPanel, ViewportBuilder,
};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailedFetchItem {
    url: String,
    title: String,
    category: String,
    message: String,
}

#[derive(Debug, Clone)]
struct ImportProgress {
    phase: String,
    processed: usize,
    total: Option<usize>,
    saved_count: usize,
    skipped_existing: usize,
    skipped_out_of_range: usize,
    failed_count: usize,
    current_item: String,
}

struct ContentRefreshResult {
    imported_urls: Result<HashSet<String>, String>,
    library_articles: Result<Vec<StoredArticle>, String>,
    library_stats: Result<LibraryStats, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JobKind {
    Import,
    Upload,
}

impl JobKind {
    fn label(self) -> &'static str {
        match self {
            Self::Import => "Import",
            Self::Upload => "LingQ upload",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadFailure {
    article_id: i64,
    title: String,
    message: String,
}

#[derive(Debug, Clone)]
struct UploadProgress {
    processed: usize,
    total: usize,
    uploaded: usize,
    failed_count: usize,
    current_item: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum QueuedJobRequest {
    Import {
        urls: Vec<String>,
    },
    Upload {
        ids: Vec<i64>,
        collection_id: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedJob {
    id: u64,
    kind: JobKind,
    label: String,
    total: usize,
    request: QueuedJobRequest,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompletedJob {
    id: u64,
    kind: JobKind,
    label: String,
    summary: String,
    success: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedQueueState {
    next_job_id: u64,
    queue_paused: bool,
    queued_jobs: Vec<QueuedJob>,
    completed_jobs: Vec<CompletedJob>,
    failed_fetches: Vec<FailedFetchItem>,
    failed_uploads: Vec<UploadFailure>,
}

enum AppEvent {
    BrowseLoaded {
        request_id: u64,
        result: Result<BrowseSectionResult, String>,
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
    browse_articles: Vec<ArticleSummary>,
    browse_report: Option<DiscoveryReport>,
    browse_scope_label: String,
    browse_selected: HashSet<String>,
    browse_only_new: bool,
    browse_imported_urls: HashSet<String>,
    browse_loading: bool,
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

impl SoziopolisLingqGui {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let (mut settings, settings_notice) = match SettingsStore::load_default() {
            Ok(settings) => (settings, None),
            Err(err) => {
                let fallback_path = std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("soziopolis_reader_settings.json");
                logging::warn(format!(
                    "could not load default settings store; using fallback settings path {}: {err}",
                    fallback_path.display()
                ));
                let fallback_settings = SettingsStore::load(fallback_path.clone())
                    .unwrap_or_else(|load_err| {
                        logging::warn(format!(
                            "could not load fallback settings file {}; using in-memory defaults instead: {load_err}",
                            fallback_path.display()
                        ));
                        SettingsStore::from_parts(fallback_path, crate::settings::AppSettings::default())
                    });
                (
                    fallback_settings,
                    Some(format!(
                        "Could not load saved settings, so the app started with defaults: {err}"
                    )),
                )
            }
        };
        let current_view = View::from_str(&settings.data().last_view);
        let browse_section = settings.data().browse_section.clone();
        let browse_only_new = settings.data().browse_only_new;
        let (lingq_api_key, startup_notice) = load_lingq_api_key_from_storage(&mut settings);
        let lingq_connected = !lingq_api_key.trim().is_empty();
        let lingq_selected_collection = settings.data().lingq_collection_id;

        let mut app = Self {
            tx,
            rx,
            settings,
            current_view,
            notice: None,
            browse_request_id: 0,
            content_refresh_request_id: 0,
            browse_section: browse_section.clone(),
            browse_limit: 80,
            browse_articles: Vec::new(),
            browse_report: None,
            browse_scope_label: "Current section".to_owned(),
            browse_selected: HashSet::new(),
            browse_only_new,
            browse_imported_urls: HashSet::new(),
            browse_loading: false,
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
        };
        app.load_persisted_queue_state();
        app.refresh_browse();
        app.request_content_refresh("app startup");
        if let Some(message) = settings_notice.or(startup_notice) {
            app.set_notice(message, NoticeKind::Info);
        }
        if app.lingq_connected {
            app.load_collections();
        }
        app.start_next_queued_job();
        app
    }

    fn save_settings(&mut self) {
        let current_view = self.current_view;
        let browse_section = self.browse_section.clone();
        let browse_only_new = self.browse_only_new;
        let lingq_collection_id = self.lingq_selected_collection;
        if let Err(err) = self.settings.update(|settings| {
            settings.last_view = current_view.as_str().to_owned();
            settings.browse_section = browse_section;
            settings.browse_only_new = browse_only_new;
            settings.lingq_collection_id = lingq_collection_id;
        }) {
            self.set_notice(
                format!("Could not save app settings: {err}"),
                NoticeKind::Error,
            );
        }
    }

    fn set_notice(&mut self, message: impl Into<String>, kind: NoticeKind) {
        let message = message.into();
        match kind {
            NoticeKind::Info => logging::info(format!("notice: {message}")),
            NoticeKind::Success => logging::info(format!("success: {message}")),
            NoticeKind::Error => logging::error(format!("notice: {message}")),
        }
        self.notice = Some(Notice {
            message,
            kind,
            created_at: Instant::now(),
        });
    }

    fn guard_ui_phase(&mut self, phase: &str, run: impl FnOnce(&mut Self)) {
        if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| run(self))) {
            self.recover_from_ui_panic(phase, payload);
        }
    }

    fn recover_from_ui_panic(&mut self, phase: &str, payload: Box<dyn Any + Send>) {
        let payload_message = panic_payload_message(payload.as_ref());
        let log_path = logging::log_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "the app log".to_owned());
        logging::error(format!(
            "recovered UI panic while {phase}: {payload_message}"
        ));
        self.batch_fetching = false;
        self.preview_loading = false;
        self.import_progress = None;
        self.show_preview = false;
        self.set_notice(
            format!(
                "The app recovered from an internal error while {phase}. Details were written to {log_path}."
            ),
            NoticeKind::Error,
        );
    }

    fn refresh_after_content_change(&mut self, reason: &str) {
        self.request_content_refresh(reason);
    }

    fn request_content_refresh(&mut self, reason: &str) {
        self.library_loading = true;
        self.content_refresh_request_id = self.content_refresh_request_id.wrapping_add(1);
        let request_id = self.content_refresh_request_id;
        let reason = reason.to_owned();
        let tx = self.tx.clone();
        logging::info(format!(
            "starting content refresh pipeline {request_id} after {reason}"
        ));
        std::thread::spawn(move || {
            let event = match panic::catch_unwind(AssertUnwindSafe(|| {
                build_content_refresh_event(request_id, reason)
            })) {
                Ok(event) => event,
                Err(payload) => {
                    let message = format!(
                        "Content refresh worker hit an internal error: {}",
                        panic_payload_message(payload.as_ref())
                    );
                    logging::error(&message);
                    AppEvent::ContentRefreshCompleted {
                        request_id,
                        reason: "internal refresh error".to_owned(),
                        result: ContentRefreshResult {
                            imported_urls: Err(message.clone()),
                            library_articles: Err(message.clone()),
                            library_stats: Err(message),
                        },
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    fn refresh_browse(&mut self) {
        self.browse_scope_label = "Current section".to_owned();
        logging::info(format!(
            "browse refresh requested for section '{}' with limit {}",
            self.browse_section, self.browse_limit
        ));
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let section = self.browse_section.clone();
        let limit = self.browse_limit;
        std::thread::spawn(move || {
            let result = (|| {
                let scraper = SoziopolisClient::new().map_err(|err| err.to_string())?;
                let section_ref = scraper
                    .section_by_id(&section)
                    .ok_or_else(|| format!("unknown section '{section}'"))?;
                scraper
                    .browse_section_detailed(section_ref, limit)
                    .map_err(|err| err.to_string())
            })();
            let _ = tx.send(AppEvent::BrowseLoaded { request_id, result });
        });
    }

    fn discover_new_across_sections(&mut self) {
        self.browse_scope_label = "All sections discovery".to_owned();
        logging::info(format!(
            "discover new across sections requested with total limit {}",
            self.browse_limit
        ));
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let limit = self.browse_limit;
        std::thread::spawn(move || {
            let result = (|| {
                let scraper = SoziopolisClient::new().map_err(|err| err.to_string())?;
                scraper
                    .browse_all_sections_detailed(limit)
                    .map_err(|err| err.to_string())
            })();
            let _ = tx.send(AppEvent::BrowseLoaded { request_id, result });
        });
    }

    fn persist_lingq_api_key(&mut self) -> bool {
        let api_key = self.lingq_api_key.trim().to_owned();
        if api_key.is_empty() {
            self.set_notice("Enter a LingQ token first.", NoticeKind::Error);
            return false;
        }

        match credential_store::save_lingq_api_key(&api_key) {
            Ok(()) => {
                self.lingq_api_key = api_key;
                if let Err(err) = self.settings.clear_legacy_lingq_api_key() {
                    self.set_notice(
                        format!(
                            "Saved the LingQ token securely, but could not clear the old settings copy: {err}"
                        ),
                        NoticeKind::Error,
                    );
                }
                self.save_settings();
                true
            }
            Err(err) => {
                self.set_notice(
                    format!("Could not save the LingQ token securely: {err}"),
                    NoticeKind::Error,
                );
                false
            }
        }
    }

    fn clear_stored_lingq_api_key(&mut self) -> bool {
        match credential_store::clear_lingq_api_key() {
            Ok(()) => {
                self.lingq_api_key.clear();
                if let Err(err) = self.settings.clear_legacy_lingq_api_key() {
                    self.set_notice(
                        format!(
                            "Cleared the stored LingQ token, but could not remove the old settings copy: {err}"
                        ),
                        NoticeKind::Error,
                    );
                }
                self.save_settings();
                true
            }
            Err(err) => {
                self.set_notice(
                    format!("Could not remove the stored LingQ token: {err}"),
                    NoticeKind::Error,
                );
                false
            }
        }
    }

    fn open_preview(&mut self, url: String) {
        logging::info(format!("opening remote preview for {}", url));
        self.preview_loading = true;
        self.show_preview = true;
        self.preview_article = None;
        self.preview_stored_article = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let scraper = SoziopolisClient::new().map_err(|err| err.to_string())?;
                scraper.fetch_article(&url).map_err(|err| err.to_string())
            })();
            let _ = tx.send(AppEvent::PreviewLoaded(result));
        });
    }

    fn open_library_preview(&mut self, article: StoredArticle) {
        logging::info(format!(
            "opening stored preview for article #{}",
            article.id
        ));
        self.preview_loading = false;
        self.show_preview = true;
        self.preview_article = Some(stored_article_to_preview_article(&article));
        self.preview_stored_article = Some(article);
    }

    fn select_all_visible_articles(&mut self) {
        match self.filtered_library_articles() {
            Ok(articles) => {
                self.lingq_selected_articles =
                    articles.into_iter().map(|article| article.id).collect();
            }
            Err(err) => self.set_notice(err, NoticeKind::Error),
        }
    }

    fn next_job_id(&mut self) -> u64 {
        self.next_job_id = self.next_job_id.wrapping_add(1);
        self.next_job_id
    }

    fn load_persisted_queue_state(&mut self) {
        let Ok(path) = app_paths::queue_state_path() else {
            logging::warn("could not resolve queue state path during startup");
            return;
        };

        if !path.exists() {
            return;
        }

        match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<PersistedQueueState>(&raw) {
                Ok(state) => {
                    self.next_job_id = self.next_job_id.max(state.next_job_id);
                    self.queue_paused = state.queue_paused;
                    self.queued_jobs = state.queued_jobs.into();
                    self.completed_jobs = state.completed_jobs.into();
                    self.failed_fetches = state.failed_fetches;
                    self.last_failed_uploads = state.failed_uploads;
                    if !self.queued_jobs.is_empty() {
                        logging::info(format!(
                            "restored {} queued job(s) from {}",
                            self.queued_jobs.len(),
                            path.display()
                        ));
                        self.set_notice(
                            format!(
                                "Restored {} queued job(s) from the last session.",
                                self.queued_jobs.len()
                            ),
                            NoticeKind::Info,
                        );
                    }
                }
                Err(err) => logging::warn(format!(
                    "could not parse queue state {}: {err}",
                    path.display()
                )),
            },
            Err(err) => logging::warn(format!(
                "could not read queue state {}: {err}",
                path.display()
            )),
        }
    }

    fn persist_queue_state(&self) {
        let Ok(path) = app_paths::queue_state_path() else {
            logging::warn("could not resolve queue state path");
            return;
        };

        let state = PersistedQueueState {
            next_job_id: self.next_job_id,
            queue_paused: self.queue_paused,
            queued_jobs: self.queued_jobs.iter().cloned().collect(),
            completed_jobs: self.completed_jobs.iter().cloned().collect(),
            failed_fetches: self.failed_fetches.clone(),
            failed_uploads: self.last_failed_uploads.clone(),
        };

        match serde_json::to_string_pretty(&state) {
            Ok(raw) => {
                if let Err(err) = fs::write(&path, raw) {
                    logging::warn(format!(
                        "could not write queue state {}: {err}",
                        path.display()
                    ));
                }
            }
            Err(err) => logging::warn(format!("could not serialize queue state: {err}")),
        }
    }

    fn enqueue_import_job(&mut self, urls: Vec<String>) {
        if urls.is_empty() {
            self.set_notice("Select at least one article first.", NoticeKind::Error);
            return;
        }
        let total = urls.len();
        let job = QueuedJob {
            id: self.next_job_id(),
            kind: JobKind::Import,
            label: format!("Import {} article(s)", total),
            total,
            request: QueuedJobRequest::Import { urls },
        };
        self.enqueue_job(job);
    }

    fn enqueue_upload_job(&mut self, ids: Vec<i64>, collection_id: Option<i64>) {
        if ids.is_empty() {
            self.set_notice(
                "Select at least one saved article to upload.",
                NoticeKind::Error,
            );
            return;
        }
        let total = ids.len();
        let job = QueuedJob {
            id: self.next_job_id(),
            kind: JobKind::Upload,
            label: format!("Upload {} article(s) to LingQ", total),
            total,
            request: QueuedJobRequest::Upload { ids, collection_id },
        };
        self.enqueue_job(job);
    }

    fn enqueue_job(&mut self, job: QueuedJob) {
        logging::info(format!("enqueueing {} job #{}", job.kind.label(), job.id));
        if self.active_job.is_some() {
            self.queued_jobs.push_back(job);
            self.persist_queue_state();
            self.set_notice(
                format!("Job queued. {} job(s) waiting.", self.queued_jobs.len()),
                NoticeKind::Info,
            );
            return;
        }
        self.start_job(job);
        self.persist_queue_state();
    }

    fn can_start_job(&mut self, job: &QueuedJob) -> bool {
        if matches!(job.request, QueuedJobRequest::Upload { .. })
            && self.lingq_api_key.trim().is_empty()
        {
            self.set_notice(
                "Queued LingQ uploads are waiting for you to connect to LingQ again.",
                NoticeKind::Info,
            );
            return false;
        }

        true
    }

    fn start_next_queued_job(&mut self) {
        if self.active_job.is_some() {
            return;
        }
        if self.queue_paused {
            return;
        }
        if let Some(job) = self.queued_jobs.pop_front() {
            if !self.can_start_job(&job) {
                self.queued_jobs.push_front(job);
                self.persist_queue_state();
                return;
            }
            self.persist_queue_state();
            self.start_job(job);
        }
    }

    fn start_job(&mut self, job: QueuedJob) {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_job = Some(ActiveJob {
            id: job.id,
            kind: job.kind,
            label: job.label.clone(),
            total: job.total,
            processed: 0,
            succeeded: 0,
            failed: 0,
            current_item: String::new(),
            cancel_flag: cancel_flag.clone(),
        });
        self.batch_fetching = job.kind == JobKind::Import;
        self.lingq_uploading = job.kind == JobKind::Upload;
        if job.kind == JobKind::Import {
            self.failed_fetches.clear();
            self.import_progress = Some(ImportProgress {
                phase: "Importing selected articles".to_owned(),
                processed: 0,
                total: Some(job.total),
                saved_count: 0,
                skipped_existing: 0,
                skipped_out_of_range: 0,
                failed_count: 0,
                current_item: String::new(),
            });
        } else {
            self.upload_progress = Some(UploadProgress {
                processed: 0,
                total: job.total,
                uploaded: 0,
                failed_count: 0,
                current_item: String::new(),
            });
        }

        match job.request {
            QueuedJobRequest::Import { urls } => self.spawn_import_job(job.id, urls, cancel_flag),
            QueuedJobRequest::Upload { ids, collection_id } => self.spawn_upload_job(
                job.id,
                ids,
                self.lingq_api_key.clone(),
                collection_id,
                cancel_flag,
            ),
        }
    }

    fn cancel_active_job(&mut self) {
        let Some(active_job) = &self.active_job else {
            self.set_notice("There is no running job to cancel.", NoticeKind::Info);
            return;
        };
        active_job.cancel_flag.store(true, Ordering::Relaxed);
        self.set_notice(
            format!("Cancel requested for {}.", active_job.label),
            NoticeKind::Info,
        );
    }

    fn pause_queue(&mut self) {
        if self.queue_paused {
            self.set_notice("The queue is already paused.", NoticeKind::Info);
            return;
        }

        self.queue_paused = true;
        self.persist_queue_state();
        self.set_notice(
            "Queue paused. The current job can finish, but no queued job will auto-start afterward.",
            NoticeKind::Info,
        );
    }

    fn resume_queue(&mut self) {
        if !self.queue_paused {
            self.set_notice("The queue is already running.", NoticeKind::Info);
            self.start_next_queued_job();
            return;
        }

        self.queue_paused = false;
        self.persist_queue_state();
        self.set_notice("Queue resumed.", NoticeKind::Success);
        self.start_next_queued_job();
    }

    fn run_queued_upload_now(&mut self) {
        if self.active_job.is_some() {
            self.set_notice(
                "A job is already running. Wait for it to finish or cancel it first.",
                NoticeKind::Info,
            );
            return;
        }
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Connect to LingQ first.", NoticeKind::Error);
            return;
        }

        let Some(upload_index) = self
            .queued_jobs
            .iter()
            .position(|job| matches!(job.request, QueuedJobRequest::Upload { .. }))
        else {
            self.set_notice("There is no queued LingQ upload to run.", NoticeKind::Info);
            return;
        };

        let Some(job) = self.queued_jobs.remove(upload_index) else {
            self.set_notice(
                "Could not pull the queued upload job forward.",
                NoticeKind::Error,
            );
            return;
        };

        if !self.can_start_job(&job) {
            self.queued_jobs.push_front(job);
            self.persist_queue_state();
            return;
        }

        self.persist_queue_state();
        self.set_notice(
            format!("Starting queued upload now: {}.", job.label),
            NoticeKind::Info,
        );
        self.start_job(job);
        self.persist_queue_state();
    }

    fn record_completed_job(
        &mut self,
        id: u64,
        kind: JobKind,
        label: String,
        summary: String,
        success: bool,
    ) {
        self.completed_jobs.push_front(CompletedJob {
            id,
            kind,
            label,
            summary,
            success,
        });
        while self.completed_jobs.len() > 10 {
            self.completed_jobs.pop_back();
        }
        self.persist_queue_state();
    }

    fn batch_fetch_selected(&mut self) {
        let urls = self.browse_selected.iter().cloned().collect::<Vec<_>>();
        self.enqueue_import_job(urls);
    }

    fn spawn_import_job(&self, job_id: u64, urls: Vec<String>, cancel_flag: Arc<AtomicBool>) {
        logging::info(format!("starting import worker for job #{job_id}"));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match panic::catch_unwind(AssertUnwindSafe(|| {
                let result = (|| {
                    let scraper = SoziopolisClient::new().map_err(|err| err.to_string())?;
                    let db = Database::open_default().map_err(|err| err.to_string())?;
                    let mut saved_count = 0;
                    let mut failed = Vec::new();
                    let total = urls.len();
                    let mut canceled = false;

                    for (index, url) in urls.into_iter().enumerate() {
                        if cancel_flag.load(Ordering::Relaxed) {
                            canceled = true;
                            break;
                        }

                        match scraper.fetch_article(&url) {
                            Ok(article) => match db.save_article(&article) {
                                Ok(_) => saved_count += 1,
                                Err(err) => failed.push(FailedFetchItem {
                                    url: url.clone(),
                                    title: article.title.clone(),
                                    category: "database".to_owned(),
                                    message: err.to_string(),
                                }),
                            },
                            Err(err) => failed.push(FailedFetchItem {
                                url: url.clone(),
                                title: String::new(),
                                category: classify_error_message(&err.to_string()),
                                message: err.to_string(),
                            }),
                        }
                        let _ = tx.send(AppEvent::BatchFetchProgress(ImportProgress {
                            phase: "Importing selected articles".to_owned(),
                            processed: index + 1,
                            total: Some(total),
                            saved_count,
                            skipped_existing: 0,
                            skipped_out_of_range: 0,
                            failed_count: failed.len(),
                            current_item: url.clone(),
                        }));
                    }
                    Ok::<_, String>((saved_count, failed, canceled))
                })();
                match result {
                    Ok((saved_count, failed, canceled)) => AppEvent::BatchFetched {
                        job_id,
                        saved_count,
                        skipped_existing: 0,
                        skipped_out_of_range: 0,
                        failed,
                        canceled,
                    },
                    Err(err) => AppEvent::BatchFetched {
                        job_id,
                        saved_count: 0,
                        skipped_existing: 0,
                        skipped_out_of_range: 0,
                        failed: vec![FailedFetchItem {
                            url: String::new(),
                            title: String::new(),
                            category: "fetch error".to_owned(),
                            message: err,
                        }],
                        canceled: false,
                    },
                }
            })) {
                Ok(event) => event,
                Err(payload) => {
                    let message = format!(
                        "Import worker hit an internal error: {}",
                        panic_payload_message(payload.as_ref())
                    );
                    logging::error(&message);
                    AppEvent::BatchFetched {
                        job_id,
                        saved_count: 0,
                        skipped_existing: 0,
                        skipped_out_of_range: 0,
                        failed: vec![FailedFetchItem {
                            url: String::new(),
                            title: String::new(),
                            category: "internal error".to_owned(),
                            message,
                        }],
                        canceled: false,
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    fn spawn_upload_job(
        &self,
        job_id: u64,
        ids: Vec<i64>,
        api_key: String,
        collection_id: Option<i64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        logging::info(format!("starting LingQ upload worker for job #{job_id}"));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match panic::catch_unwind(AssertUnwindSafe(|| {
                let result = (|| {
                    let db = Database::open_default().map_err(|err| err.to_string())?;
                    let lingq = LingqClient::new().map_err(|err| err.to_string())?;
                    let mut uploaded = 0;
                    let mut failed = Vec::new();
                    let total = ids.len();
                    let mut canceled = false;

                    for (index, id) in ids.into_iter().enumerate() {
                        if cancel_flag.load(Ordering::Relaxed) {
                            canceled = true;
                            break;
                        }

                        let Some(article) = db.get_article(id).map_err(|err| err.to_string())?
                        else {
                            failed.push(UploadFailure {
                                article_id: id,
                                title: format!("Article #{id}"),
                                message: "article not found".to_owned(),
                            });
                            continue;
                        };
                        let request = UploadRequest {
                            api_key: api_key.clone(),
                            language_code: "de".to_owned(),
                            collection_id,
                            title: article.title.clone(),
                            text: article.clean_text.clone(),
                            original_url: Some(article.url.clone()),
                        };
                        match lingq.upload_lesson(&request) {
                            Ok(response) => {
                                if let Err(err) = db.mark_uploaded(
                                    article.id,
                                    response.lesson_id,
                                    &response.lesson_url,
                                ) {
                                    failed.push(UploadFailure {
                                        article_id: article.id,
                                        title: article.title.clone(),
                                        message: format!("uploaded but DB update failed: {}", err),
                                    });
                                } else {
                                    uploaded += 1;
                                }
                            }
                            Err(err) => failed.push(UploadFailure {
                                article_id: article.id,
                                title: article.title.clone(),
                                message: err.to_string(),
                            }),
                        }
                        let _ = tx.send(AppEvent::UploadProgress {
                            job_id,
                            progress: UploadProgress {
                                processed: index + 1,
                                total,
                                uploaded,
                                failed_count: failed.len(),
                                current_item: article.title.clone(),
                            },
                        });
                    }
                    Ok::<_, String>((uploaded, failed, canceled))
                })();
                match result {
                    Ok((uploaded, failed, canceled)) => AppEvent::BatchUploaded {
                        job_id,
                        uploaded,
                        failed,
                        canceled,
                    },
                    Err(err) => AppEvent::BatchUploaded {
                        job_id,
                        uploaded: 0,
                        failed: vec![UploadFailure {
                            article_id: 0,
                            title: "Upload job".to_owned(),
                            message: err,
                        }],
                        canceled: false,
                    },
                }
            })) {
                Ok(event) => event,
                Err(payload) => {
                    let message = format!(
                        "LingQ upload worker hit an internal error: {}",
                        panic_payload_message(payload.as_ref())
                    );
                    logging::error(&message);
                    AppEvent::BatchUploaded {
                        job_id,
                        uploaded: 0,
                        failed: vec![UploadFailure {
                            article_id: 0,
                            title: "Upload job".to_owned(),
                            message,
                        }],
                        canceled: false,
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    fn retry_failed_fetches(&mut self) {
        if self.failed_fetches.is_empty() {
            self.set_notice("There are no failed items to retry.", NoticeKind::Info);
            return;
        }

        self.browse_selected = self
            .failed_fetches
            .iter()
            .map(|item| item.url.clone())
            .collect();
        self.batch_fetch_selected();
    }

    fn retry_failed_uploads(&mut self) {
        if self.last_failed_uploads.is_empty() {
            self.set_notice(
                "There are no failed LingQ uploads to retry.",
                NoticeKind::Info,
            );
            return;
        }
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Connect to LingQ first.", NoticeKind::Error);
            return;
        }

        let ids = self
            .last_failed_uploads
            .iter()
            .filter_map(|item| (item.article_id > 0).then_some(item.article_id))
            .collect::<Vec<_>>();
        self.enqueue_upload_job(ids, self.lingq_selected_collection);
    }

    fn select_lingq_articles_by_word_count(&mut self) {
        let min_words = match parse_optional_positive_usize_input(
            &self.lingq_word_count_min,
            "Minimum words",
        ) {
            Ok(value) => value,
            Err(err) => {
                self.set_notice(err, NoticeKind::Error);
                return;
            }
        };
        let max_words = match parse_optional_positive_usize_input(
            &self.lingq_word_count_max,
            "Maximum words",
        ) {
            Ok(value) => value,
            Err(err) => {
                self.set_notice(err, NoticeKind::Error);
                return;
            }
        };

        if let (Some(min_words), Some(max_words)) = (min_words, max_words) {
            if min_words > max_words {
                self.set_notice(
                    "Minimum words must be less than or equal to maximum words.",
                    NoticeKind::Error,
                );
                return;
            }
        }

        self.lingq_selected_articles = self
            .library_articles
            .iter()
            .filter(|article| {
                (!self.lingq_select_only_not_uploaded || !article.uploaded_to_lingq)
                    && min_words.is_none_or(|min| article.word_count as usize >= min)
                    && max_words.is_none_or(|max| article.word_count as usize <= max)
            })
            .map(|article| article.id)
            .collect();

        self.set_notice(
            format!(
                "Selected {} article(s) for LingQ upload.",
                self.lingq_selected_articles.len()
            ),
            NoticeKind::Info,
        );
    }

    fn browse_article_passes_new_filter(&self, article: &ArticleSummary) -> bool {
        if self.browse_imported_urls.contains(&article.url) {
            return false;
        }

        browse_article_is_recent_enough(article, latest_saved_article_date(&self.library_articles))
    }

    fn browse_article_is_visible(&self, article: &ArticleSummary) -> bool {
        !self.browse_only_new || self.browse_article_passes_new_filter(article)
    }

    fn filtered_library_articles(&self) -> Result<Vec<StoredArticle>, String> {
        let min_words = parse_optional_positive_usize_input(
            &self.library_word_count_min,
            "Library minimum words",
        )?;
        let max_words = parse_optional_positive_usize_input(
            &self.library_word_count_max,
            "Library maximum words",
        )?;

        if let (Some(min_words), Some(max_words)) = (min_words, max_words) {
            if min_words > max_words {
                return Err(
                    "Library minimum words must be less than or equal to maximum words.".to_owned(),
                );
            }
        }

        let mut articles = self
            .library_articles
            .iter()
            .filter(|article| {
                article_matches_library_search(article, &self.library_search)
                    && (self.library_topic.trim().is_empty()
                        || effective_topic_for_article(article) == self.library_topic)
                    && (!self.library_only_not_uploaded || !article.uploaded_to_lingq)
                    && min_words.is_none_or(|min| article.word_count as usize >= min)
                    && max_words.is_none_or(|max| article.word_count as usize <= max)
            })
            .cloned()
            .collect::<Vec<_>>();

        articles.sort_by(|a, b| {
            let primary = if self.library_group_by_topic {
                effective_topic_for_article(a)
                    .cmp(&effective_topic_for_article(b))
                    .then_with(|| compare_library_articles(a, b, self.library_sort_mode))
            } else {
                compare_library_articles(a, b, self.library_sort_mode)
            };

            primary.then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        Ok(articles)
    }

    fn load_collections(&mut self) {
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Enter a LingQ API key first.", NoticeKind::Error);
            return;
        }
        logging::info("loading LingQ collections");
        self.lingq_loading_collections = true;
        let tx = self.tx.clone();
        let api_key = self.lingq_api_key.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let lingq = LingqClient::new().map_err(|err| err.to_string())?;
                lingq
                    .get_collections(&api_key, "de")
                    .map_err(|err| err.to_string())
            })();
            let _ = tx.send(AppEvent::CollectionsLoaded(result));
        });
    }

    fn login_to_lingq(&mut self) {
        if self.lingq_username.trim().is_empty() || self.lingq_password.is_empty() {
            self.set_notice(
                "Enter your LingQ username/email and password.",
                NoticeKind::Error,
            );
            return;
        }
        logging::info("attempting LingQ login");
        self.lingq_loading_collections = true;
        let tx = self.tx.clone();
        let username = self.lingq_username.clone();
        let password = self.lingq_password.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let lingq = LingqClient::new().map_err(|err| err.to_string())?;
                lingq
                    .login(&username, &password)
                    .map(|login| login.token)
                    .map_err(|err| err.to_string())
            })();
            let _ = tx.send(AppEvent::LingqLoggedIn(result));
        });
    }

    fn batch_upload_selected(&mut self) {
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Connect to LingQ first.", NoticeKind::Error);
            return;
        }
        let collection_id = self.lingq_selected_collection;
        let ids = self
            .lingq_selected_articles
            .iter()
            .copied()
            .collect::<Vec<_>>();
        self.enqueue_upload_job(ids, collection_id);
    }

    fn open_article(&mut self, article: StoredArticle) {
        self.article_detail = Some(article);
        self.current_view = View::Article;
        self.save_settings();
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                AppEvent::BrowseLoaded { request_id, result } => {
                    if request_id != self.browse_request_id {
                        logging::warn(format!(
                            "discarded stale browse result for request {request_id}; current request is {}",
                            self.browse_request_id
                        ));
                        continue;
                    }
                    self.browse_loading = false;
                    match result {
                        Ok(result) => {
                            logging::info(format!(
                                "browse result loaded with {} article(s)",
                                result.articles.len()
                            ));
                            self.browse_report = Some(result.report);
                            self.browse_articles = result.articles;
                        }
                        Err(err) => self.set_notice(err, NoticeKind::Error),
                    }
                }
                AppEvent::PreviewLoaded(result) => {
                    self.preview_loading = false;
                    match result {
                        Ok(article) => {
                            logging::info(format!("preview loaded for {}", article.url));
                            self.preview_article = Some(article);
                        }
                        Err(err) => {
                            self.show_preview = false;
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                AppEvent::BatchFetchProgress(progress) => {
                    if let Some(active_job) = &mut self.active_job {
                        if active_job.kind == JobKind::Import {
                            active_job.processed = progress.processed;
                            active_job.succeeded = progress.saved_count;
                            active_job.failed = progress.failed_count;
                            active_job.current_item = progress.current_item.clone();
                        }
                    }
                    self.import_progress = Some(progress);
                }
                AppEvent::UploadProgress { job_id, progress } => {
                    if let Some(active_job) = &mut self.active_job {
                        if active_job.id == job_id && active_job.kind == JobKind::Upload {
                            active_job.processed = progress.processed;
                            active_job.total = progress.total;
                            active_job.succeeded = progress.uploaded;
                            active_job.failed = progress.failed_count;
                            active_job.current_item = progress.current_item.clone();
                        }
                    }
                    self.upload_progress = Some(progress);
                }
                AppEvent::BatchFetched {
                    job_id,
                    saved_count,
                    skipped_existing,
                    skipped_out_of_range,
                    failed,
                    canceled,
                } => {
                    let job_label = self
                        .active_job
                        .as_ref()
                        .map(|job| job.label.clone())
                        .unwrap_or_else(|| "Import job".to_owned());
                    self.batch_fetching = false;
                    self.import_progress = None;
                    self.failed_fetches = failed.clone();
                    self.refresh_after_content_change("batch import");
                    self.record_completed_job(
                        job_id,
                        JobKind::Import,
                        job_label,
                        format!(
                            "Saved {saved_count}, failed {}, canceled {}",
                            failed.len(),
                            if canceled { "yes" } else { "no" }
                        ),
                        failed.is_empty() && !canceled,
                    );
                    self.active_job = None;
                    self.start_next_queued_job();
                    self.persist_queue_state();
                    if failed.is_empty() {
                        self.set_notice(
                            if canceled {
                                format!("Import canceled after saving {saved_count} article(s).")
                            } else {
                                format!(
                                    "Saved {saved_count} article(s); skipped {skipped_existing} already imported and {skipped_out_of_range} outside the date window."
                                )
                            },
                            if canceled {
                                NoticeKind::Info
                            } else {
                                NoticeKind::Success
                            },
                        );
                    } else {
                        self.set_notice(
                            format!(
                                "{} Saved {saved_count} article(s); skipped {skipped_existing} already imported, {skipped_out_of_range} outside the date window, and {} failed. First error: {}",
                                if canceled { "Import canceled." } else { "" },
                                failed.len(),
                                failed[0].message
                            ),
                            if saved_count > 0 {
                                NoticeKind::Info
                            } else {
                                NoticeKind::Error
                            },
                        );
                    }
                }
                AppEvent::ContentRefreshCompleted {
                    request_id,
                    reason,
                    result,
                } => {
                    if request_id != self.content_refresh_request_id {
                        logging::warn(format!(
                            "discarded stale content refresh result for request {request_id}; current request is {}",
                            self.content_refresh_request_id
                        ));
                        continue;
                    }
                    self.library_loading = false;

                    let mut failures = Vec::new();

                    match result.imported_urls {
                        Ok(urls) => {
                            logging::info(format!(
                                "content refresh {request_id}: imported URL cache refreshed with {} entries",
                                urls.len()
                            ));
                            self.browse_imported_urls = urls;
                        }
                        Err(err) => failures.push(format!("imported URL cache: {err}")),
                    }

                    match result.library_articles {
                        Ok(articles) => {
                            logging::info(format!(
                                "content refresh {request_id}: library refreshed with {} article(s)",
                                articles.len()
                            ));
                            self.library_articles = articles;
                        }
                        Err(err) => failures.push(format!("library articles: {err}")),
                    }

                    match result.library_stats {
                        Ok(stats) => {
                            logging::info(format!(
                                "content refresh {request_id}: stats refreshed; articles={}, uploaded={}, avg_words={}",
                                stats.total_articles,
                                stats.uploaded_articles,
                                stats.average_word_count
                            ));
                            self.library_stats = Some(stats);
                        }
                        Err(err) => failures.push(format!("library stats: {err}")),
                    }

                    if !failures.is_empty() {
                        logging::error(format!(
                            "content refresh after {reason} completed with {} issue(s): {}",
                            failures.len(),
                            failures.join(" | ")
                        ));
                        self.set_notice(
                            format!(
                                "Refresh after {reason} finished with {} issue(s). First error: {}",
                                failures.len(),
                                failures[0]
                            ),
                            if failures.len() == 3 {
                                NoticeKind::Error
                            } else {
                                NoticeKind::Info
                            },
                        );
                    }
                }
                AppEvent::LingqLoggedIn(result) => match result {
                    Ok(token) => {
                        self.lingq_api_key = token;
                        self.lingq_password.clear();
                        if self.persist_lingq_api_key() {
                            self.load_collections();
                        } else {
                            self.lingq_loading_collections = false;
                        }
                    }
                    Err(err) => {
                        self.lingq_loading_collections = false;
                        self.set_notice(err, NoticeKind::Error);
                    }
                },
                AppEvent::CollectionsLoaded(result) => {
                    self.lingq_loading_collections = false;
                    match result {
                        Ok(collections) => {
                            self.lingq_collections = collections;
                            self.lingq_connected = true;
                            self.save_settings();
                            self.start_next_queued_job();
                            self.set_notice("Connected to LingQ.", NoticeKind::Success);
                        }
                        Err(err) => {
                            self.lingq_connected = false;
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                AppEvent::BatchUploaded {
                    job_id,
                    uploaded,
                    failed,
                    canceled,
                } => {
                    let job_label = self
                        .active_job
                        .as_ref()
                        .map(|job| job.label.clone())
                        .unwrap_or_else(|| "Upload job".to_owned());
                    self.lingq_uploading = false;
                    self.upload_progress = None;
                    self.refresh_after_content_change("LingQ batch upload");
                    self.lingq_selected_articles.clear();
                    self.last_failed_uploads = failed.clone();
                    self.record_completed_job(
                        job_id,
                        JobKind::Upload,
                        job_label,
                        format!(
                            "Uploaded {uploaded}, failed {}, canceled {}",
                            failed.len(),
                            if canceled { "yes" } else { "no" }
                        ),
                        failed.is_empty() && !canceled,
                    );
                    self.active_job = None;
                    self.start_next_queued_job();
                    self.persist_queue_state();
                    if failed.is_empty() {
                        self.set_notice(
                            if canceled {
                                format!("Upload canceled after {uploaded} successful article(s).")
                            } else {
                                format!("Uploaded {uploaded} article(s).")
                            },
                            if canceled {
                                NoticeKind::Info
                            } else {
                                NoticeKind::Success
                            },
                        );
                    } else {
                        self.set_notice(
                            format!(
                                "{} Uploaded {uploaded} article(s); {} failed. First error: {}",
                                if canceled { "Upload canceled." } else { "" },
                                failed.len(),
                                if failed[0].title.is_empty() {
                                    failed[0].message.clone()
                                } else {
                                    format!("{}: {}", failed[0].title, failed[0].message)
                                }
                            ),
                            if uploaded > 0 {
                                NoticeKind::Info
                            } else {
                                NoticeKind::Error
                            },
                        );
                    }
                }
            }
        }

        if let Some(notice) = &self.notice {
            if notice.created_at.elapsed() > Duration::from_secs(7) {
                self.notice = None;
            }
        }
    }

    fn sidebar(&mut self, ctx: &Context) {
        SidePanel::left("sidebar")
            .exact_width(240.0)
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(24, 28, 37))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.heading(RichText::new("Soziopolis Reader").size(24.0).strong());
                ui.label(RichText::new("soziopolis.de + LingQ").color(Color32::from_gray(160)));
                ui.add_space(20.0);

                for (view, label) in [
                    (View::Browse, "Browse Articles"),
                    (View::Library, "My Library"),
                    (View::Diagnostics, "Diagnostics"),
                ] {
                    if ui
                        .selectable_label(self.current_view == view, label)
                        .clicked()
                    {
                        self.current_view = view;
                        self.save_settings();
                    }
                }

                ui.add_space(8.0);
                if ui.button("LingQ Settings").clicked() {
                    self.show_lingq_settings = true;
                }

                if self.current_view == View::Library {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(RichText::new("Library Stats").strong());
                    ui.add_space(6.0);
                    if let Some(stats) = &self.library_stats {
                        sidebar_stat_row(ui, "Articles", stats.total_articles);
                        sidebar_stat_row(ui, "Uploaded", stats.uploaded_articles);
                        sidebar_stat_row(ui, "Avg words", stats.average_word_count);
                    } else {
                        ui.small(
                            RichText::new("Loading library stats...")
                                .color(Color32::from_gray(150)),
                        );
                    }
                }

                if self.current_view == View::Browse {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(RichText::new("Failed Imports").strong());
                    ui.small(format!(
                        "{} retained failed item(s).",
                        self.failed_fetches.len()
                    ));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                !self.failed_fetches.is_empty() && !self.batch_fetching,
                                egui::Button::new("Retry"),
                            )
                            .clicked()
                        {
                            self.retry_failed_fetches();
                        }
                        if ui
                            .add_enabled(
                                !self.failed_fetches.is_empty(),
                                egui::Button::new("Clear"),
                            )
                            .clicked()
                        {
                            self.failed_fetches.clear();
                        }
                    });
                    ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        if self.failed_fetches.is_empty() {
                            ui.small(
                                RichText::new("No failed imports right now.")
                                    .color(Color32::from_gray(150)),
                            );
                        } else {
                            for item in &self.failed_fetches {
                                ui.small(
                                    RichText::new(format!(
                                        "[{}] {}",
                                        item.category,
                                        if item.title.is_empty() {
                                            &item.url
                                        } else {
                                            &item.title
                                        }
                                    ))
                                    .monospace(),
                                );
                            }
                        }
                    });
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.separator();
                    ui.label(
                        RichText::new(if self.lingq_connected {
                            "LingQ: Connected"
                        } else {
                            "LingQ: Not connected"
                        })
                        .color(if self.lingq_connected {
                            Color32::from_rgb(94, 214, 130)
                        } else {
                            Color32::from_rgb(238, 100, 100)
                        }),
                    );
                    ui.label(
                        RichText::new("Soziopolis: Public access").color(Color32::from_gray(180)),
                    );
                });
            });
    }

    fn top_notice(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_notice")
            .exact_height(if self.notice.is_some() { 36.0 } else { 0.0 })
            .show(ctx, |ui| {
                if let Some(notice) = &self.notice {
                    let color = match notice.kind {
                        NoticeKind::Info => Color32::from_rgb(92, 135, 255),
                        NoticeKind::Success => Color32::from_rgb(94, 214, 130),
                        NoticeKind::Error => Color32::from_rgb(238, 100, 100),
                    };
                    ui.label(RichText::new(&notice.message).color(color));
                }
            });
    }

    fn lingq_settings_window(&mut self, ctx: &Context) {
        if !self.show_lingq_settings {
            return;
        }

        let mut open = self.show_lingq_settings;
        egui::Window::new("LingQ Settings")
            .open(&mut open)
            .default_width(620.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(
                    "Manage your LingQ login or token here. The library page stays focused on selecting and uploading saved articles.",
                );
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.lingq_auth_mode,
                        LingqAuthMode::Account,
                        "Account Login",
                    );
                    ui.selectable_value(
                        &mut self.lingq_auth_mode,
                        LingqAuthMode::Token,
                        "Token / API Key",
                    );
                });
                ui.add_space(8.0);

                if self.lingq_auth_mode == LingqAuthMode::Account {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Username or email");
                        ui.add(TextEdit::singleline(&mut self.lingq_username).desired_width(220.0));
                        ui.label("Password");
                        ui.add(
                            TextEdit::singleline(&mut self.lingq_password)
                                .password(true)
                                .desired_width(180.0),
                        );
                        if ui.button("Sign in").clicked() {
                            self.login_to_lingq();
                        }
                        if self.lingq_loading_collections {
                            ui.spinner();
                        }
                    });
                    ui.small(
                        "The app signs in to LingQ, retrieves your token, and stores that token in Windows Credential Manager for future uploads.",
                    );
                    ui.add_space(10.0);
                }

                ui.horizontal_wrapped(|ui| {
                    ui.label("Token / API key");
                    ui.add(
                        TextEdit::singleline(&mut self.lingq_api_key)
                            .password(true)
                            .desired_width(320.0),
                    );
                    if ui.button("Connect").clicked() {
                        if self.persist_lingq_api_key() {
                            self.load_collections();
                        }
                    }
                    if ui.button("Disconnect").clicked() {
                        if self.clear_stored_lingq_api_key() {
                            self.lingq_connected = false;
                            self.lingq_collections.clear();
                        }
                    }
                    if self.lingq_loading_collections {
                        ui.spinner();
                    }
                });
                ui.small("This token is stored securely in Windows Credential Manager instead of plain JSON settings.");

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(if self.lingq_connected {
                            "Status: Connected"
                        } else {
                            "Status: Not connected"
                        })
                        .color(if self.lingq_connected {
                            Color32::from_rgb(94, 214, 130)
                        } else {
                            Color32::from_rgb(238, 100, 100)
                        }),
                    );
                    if ui.button("Refresh destinations").clicked() {
                        self.load_collections();
                    }
                });
            });
        self.show_lingq_settings = open;
    }

    fn browse_view(&mut self, ui: &mut egui::Ui) {
        let available_sections = SoziopolisClient::new()
            .ok()
            .map(|client| client.sections().to_vec())
            .unwrap_or_default();
        let imported_count = self
            .browse_articles
            .iter()
            .filter(|article| self.browse_imported_urls.contains(&article.url))
            .count();
        let date_filtered_count = self
            .browse_articles
            .iter()
            .filter(|article| {
                !self.browse_imported_urls.contains(&article.url)
                    && !browse_article_is_recent_enough(
                        article,
                        latest_saved_article_date(&self.library_articles),
                    )
            })
            .count();
        let new_count = self
            .browse_articles
            .iter()
            .filter(|article| self.browse_article_passes_new_filter(article))
            .count();
        let visible_articles = self
            .browse_articles
            .iter()
            .filter(|article| self.browse_article_is_visible(article))
            .cloned()
            .collect::<Vec<_>>();
        let latest_saved_date = latest_saved_article_date(&self.library_articles);

        framed_panel(ui, |ui| {
            let previous_section = self.browse_section.clone();
            ui.horizontal_wrapped(|ui| {
                ui.label("Section");
                egui::ComboBox::from_id_salt("browse_section")
                    .selected_text(
                        available_sections
                            .iter()
                            .find(|section| section.id == self.browse_section)
                            .map(|section| section.label)
                            .unwrap_or(self.browse_section.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for section in &available_sections {
                            ui.selectable_value(
                                &mut self.browse_section,
                                section.id.to_owned(),
                                section.label,
                            );
                        }
                    });
                if self.browse_section != previous_section {
                    self.browse_limit = 80;
                    self.save_settings();
                    self.refresh_browse();
                }
                ui.label(format!("Limit: {}", self.browse_limit));
                if ui.button("Refresh").clicked() {
                    self.save_settings();
                    self.refresh_browse();
                }
                if ui.button("Find new across sections").clicked() {
                    self.browse_only_new = true;
                    self.save_settings();
                    self.discover_new_across_sections();
                }
                if ui
                    .add_enabled(!self.browse_loading, egui::Button::new("Load more"))
                    .clicked()
                {
                    self.browse_limit += 80;
                    self.refresh_browse();
                }
                if ui.button("Select all not imported").clicked() {
                    self.browse_selected = self
                        .browse_articles
                        .iter()
                        .filter(|article| !self.browse_imported_urls.contains(&article.url))
                        .map(|article| article.url.clone())
                        .collect();
                }
                if ui.checkbox(&mut self.browse_only_new, "Only new").changed() {
                    self.save_settings();
                }
                if ui.button("Clear selection").clicked() {
                    self.browse_selected.clear();
                }
                if ui
                    .add_enabled(
                        !self.batch_fetching && !self.browse_selected.is_empty(),
                        egui::Button::new(format!("Fetch & Save ({})", self.browse_selected.len())),
                    )
                    .clicked()
                {
                    self.batch_fetch_selected();
                }
                if self.browse_loading {
                    ui.spinner();
                    ui.label("Loading...");
                }
                if self.batch_fetching {
                    ui.spinner();
                    ui.label("Saving...");
                }
            });

            if let Some(progress) = &self.import_progress {
                ui.add_space(10.0);
                render_import_progress(ui, progress);
            }

            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Scope: {}", self.browse_scope_label));
                ui.label(format!("Loaded {} article(s).", self.browse_articles.len()));
                ui.label(format!(
                    "{} selected, {} already imported, {} new, {} visible.",
                    self.browse_selected.len(),
                    imported_count,
                    new_count,
                    visible_articles.len()
                ));
            });
            if self.browse_only_new {
                let summary = latest_saved_date
                    .map(|date| {
                        if date_filtered_count > 0 {
                            format!(
                                "Only new is using the newest saved article date ({}) and skipped {date_filtered_count} older unseen article(s).",
                                format_naive_date(date)
                            )
                        } else {
                            format!(
                                "Only new is using the newest saved article date ({}).",
                                format_naive_date(date)
                            )
                        }
                    })
                    .unwrap_or_else(|| {
                        "Only new is falling back to the imported-URL check until the library has a usable article date.".to_owned()
                    });
                ui.small(RichText::new(summary).color(Color32::from_gray(160)));
            }
        });

        ui.add_space(12.0);
        ScrollArea::vertical().show(ui, |ui| {
            for article in visible_articles {
                article_card_frame(ui, |ui| {
                    ui.vertical(|ui| {
                        let mut checked = self.browse_selected.contains(&article.url);
                        ui.horizontal_wrapped(|ui| {
                            if ui.checkbox(&mut checked, "").changed() {
                                if checked {
                                    self.browse_selected.insert(article.url.clone());
                                } else {
                                    self.browse_selected.remove(&article.url);
                                }
                            }
                            ui.label(RichText::new(&article.title).strong().size(15.5));
                        });
                        if !article.teaser.is_empty() {
                            ui.small(
                                RichText::new(truncate_for_ui(&article.teaser, 220))
                                    .color(Color32::from_gray(188))
                                    .italics(),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            tag(ui, &article.section);
                            if !article.author.is_empty() {
                                tag(ui, &truncate_for_ui(&article.author, 28));
                            }
                            if !article.date.is_empty() {
                                tag(ui, &article.date);
                            }
                            if self.browse_imported_urls.contains(&article.url) {
                                success_tag(ui, "Imported");
                            } else if self.browse_only_new
                                && browse_article_is_recent_enough(&article, latest_saved_date)
                            {
                                success_tag(ui, "Recent");
                            }
                            if ui.link("Open original").clicked() {
                                let _ = webbrowser::open(&article.url);
                            }
                            if ui.link("Preview").clicked() {
                                self.open_preview(article.url.clone());
                            }
                        });
                        ui.small(
                            RichText::new(compact_url(&article.url)).color(Color32::from_gray(150)),
                        );
                    });
                });
                ui.add_space(4.0);
            }

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Need more from this section?");
                if ui
                    .add_enabled(!self.browse_loading, egui::Button::new("Load 80 more"))
                    .clicked()
                {
                    self.browse_limit += 80;
                    self.refresh_browse();
                }
            });
        });
    }

    fn library_view(&mut self, ui: &mut egui::Ui) {
        let filtered_articles = match self.filtered_library_articles() {
            Ok(articles) => articles,
            Err(err) => {
                self.set_notice(err, NoticeKind::Error);
                self.library_articles.clone()
            }
        };

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Library Filters").strong());
            if ui
                .button(if self.library_filters_expanded {
                    "Hide filters"
                } else {
                    "Show filters"
                })
                .clicked()
            {
                self.library_filters_expanded = !self.library_filters_expanded;
            }
        });
        if self.library_filters_expanded {
            framed_panel(ui, |ui| {
                let topic_counts = collect_topic_counts(&self.library_articles);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Search");
                    ui.add(TextEdit::singleline(&mut self.library_search).desired_width(220.0));
                    ui.label("Topic");
                    egui::ComboBox::from_id_salt("library_topic")
                        .selected_text(if self.library_topic.is_empty() {
                            "All topics".to_owned()
                        } else {
                            truncate_for_ui(&self.library_topic, 28)
                        })
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.library_topic,
                                String::new(),
                                "All topics",
                            );
                            for (topic, count) in &topic_counts {
                                ui.selectable_value(
                                    &mut self.library_topic,
                                    topic.clone(),
                                    format!("{} ({})", topic, count),
                                );
                            }
                        });
                    ui.checkbox(&mut self.library_only_not_uploaded, "Only not yet uploaded");
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Min words");
                    ui.add(
                        TextEdit::singleline(&mut self.library_word_count_min)
                            .desired_width(70.0)
                            .hint_text("e.g. 600"),
                    );
                    ui.label("Max words");
                    ui.add(
                        TextEdit::singleline(&mut self.library_word_count_max)
                            .desired_width(70.0)
                            .hint_text("e.g. 1800"),
                    );
                    egui::ComboBox::from_id_salt("library_sort_mode")
                        .selected_text(self.library_sort_mode.label())
                        .show_ui(ui, |ui| {
                            for sort_mode in [
                                LibrarySortMode::Newest,
                                LibrarySortMode::Oldest,
                                LibrarySortMode::Longest,
                                LibrarySortMode::Shortest,
                                LibrarySortMode::Title,
                            ] {
                                ui.selectable_value(
                                    &mut self.library_sort_mode,
                                    sort_mode,
                                    sort_mode.label(),
                                );
                            }
                        });
                    ui.checkbox(&mut self.library_group_by_topic, "Group by topic");
                    if ui.button("Refresh").clicked() {
                        self.request_content_refresh("manual library refresh");
                    }
                    if self.library_loading {
                        ui.spinner();
                    }
                });
            });
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Showing {} saved article(s).",
                filtered_articles.len()
            ));
            ui.label(format!(
                "{} article(s) selected for LingQ upload.",
                self.lingq_selected_articles.len()
            ));
            if ui.button("Select all visible").clicked() {
                self.select_all_visible_articles();
            }
            if ui.button("Select all not uploaded").clicked() {
                self.lingq_selected_articles = filtered_articles
                    .iter()
                    .filter(|article| !article.uploaded_to_lingq)
                    .map(|article| article.id)
                    .collect();
            }
            if ui.button("Clear selection").clicked() {
                self.lingq_selected_articles.clear();
            }
        });
        ui.add_space(6.0);
        self.lingq_panel(ui);
        ui.add_space(8.0);
        ScrollArea::vertical().show(ui, |ui| {
            if self.library_group_by_topic {
                let topic_counts = collect_topic_counts(&filtered_articles);
                let mut current_topic = String::new();
                for article in filtered_articles.clone() {
                    let article_topic = effective_topic_for_article(&article);
                    if article_topic != current_topic {
                        current_topic = article_topic.clone();
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} ({})",
                                    current_topic,
                                    topic_counts.get(&current_topic).copied().unwrap_or(0)
                                ))
                                .strong()
                                .size(16.5),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                    }
                    render_library_article_card(self, ui, article);
                    ui.add_space(4.0);
                }
            } else {
                for article in filtered_articles {
                    render_library_article_card(self, ui, article);
                    ui.add_space(4.0);
                }
            }
        });
    }

    fn lingq_panel(&mut self, ui: &mut egui::Ui) {
        if !self.lingq_connected {
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.label("Course");
            let previous_collection = self.lingq_selected_collection;
            egui::ComboBox::from_id_salt("lingq_collection")
                .selected_text(
                    self.lingq_selected_collection
                        .and_then(|id| {
                            self.lingq_collections
                                .iter()
                                .find(|collection| collection.id == id)
                                .map(|collection| collection.title.clone())
                        })
                        .unwrap_or_else(|| "Standalone lesson".to_owned()),
                )
                .width(240.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.lingq_selected_collection,
                        None,
                        "Standalone lesson",
                    );
                    for collection in &self.lingq_collections {
                        ui.selectable_value(
                            &mut self.lingq_selected_collection,
                            Some(collection.id),
                            format!("{} ({})", collection.title, collection.lessons_count),
                        );
                    }
                });
            if self.lingq_selected_collection != previous_collection {
                self.save_settings();
            }
            if ui.button("Refresh courses").clicked() {
                self.load_collections();
            }
            if ui.button("Select not uploaded").clicked() {
                self.lingq_selected_articles = self
                    .filtered_library_articles()
                    .unwrap_or_else(|_| self.library_articles.clone())
                    .iter()
                    .filter(|article| !article.uploaded_to_lingq)
                    .map(|article| article.id)
                    .collect();
            }
            ui.label("Min");
            ui.add(
                TextEdit::singleline(&mut self.lingq_word_count_min)
                    .desired_width(62.0)
                    .hint_text("600"),
            );
            ui.label("Max");
            ui.add(
                TextEdit::singleline(&mut self.lingq_word_count_max)
                    .desired_width(62.0)
                    .hint_text("1800"),
            );
            ui.checkbox(
                &mut self.lingq_select_only_not_uploaded,
                "Only not uploaded",
            );
            if ui.button("Select by words").clicked() {
                self.select_lingq_articles_by_word_count();
            }
            if ui.button("Clear upload selection").clicked() {
                self.lingq_selected_articles.clear();
            }
            if ui
                .add_enabled(
                    !self.lingq_uploading && !self.lingq_selected_articles.is_empty(),
                    egui::Button::new(format!("Upload {}", self.lingq_selected_articles.len())),
                )
                .clicked()
            {
                self.save_settings();
                self.batch_upload_selected();
            }
            if self.lingq_uploading {
                ui.spinner();
                ui.label("Uploading...");
            }
        });
    }

    fn diagnostics_view(&mut self, ui: &mut egui::Ui) {
        let data_dir = app_paths::data_dir().ok();
        let log_path = app_paths::app_log_path().ok();
        let exe_path = std::env::current_exe().ok();

        ui.heading("Diagnostics");
        ui.add_space(8.0);
        framed_panel(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                if let Some(path) = &data_dir {
                    ui.label(format!("Data: {}", path.display()));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open data folder").clicked() {
                    if let Some(path) = &data_dir {
                        if let Err(err) = open_path_in_explorer(path) {
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                if ui.button("Open log file").clicked() {
                    if let Some(path) = &log_path {
                        if let Err(err) = open_log_in_notepad(path) {
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                if ui.button("Copy recent log").clicked() {
                    match read_recent_log_excerpt(30) {
                        Ok(text) => {
                            ui.ctx().copy_text(text);
                            self.set_notice("Copied recent log lines.", NoticeKind::Success);
                        }
                        Err(err) => self.set_notice(err, NoticeKind::Error),
                    }
                }
                if ui.button("Create support bundle").clicked() {
                    match create_support_bundle(self) {
                        Ok(path) => {
                            if let Err(err) = open_path_in_explorer(&path) {
                                self.set_notice(
                                    format!(
                                        "Created support bundle at {}, but could not open it: {err}",
                                        path.display()
                                    ),
                                    NoticeKind::Info,
                                );
                            } else {
                                self.set_notice(
                                    format!("Created support bundle at {}.", path.display()),
                                    NoticeKind::Success,
                                );
                            }
                        }
                        Err(err) => self.set_notice(err, NoticeKind::Error),
                    }
                }
            });
            if let Some(path) = &exe_path {
                ui.small(
                    RichText::new(format!("Executable: {}", path.display()))
                        .color(Color32::from_gray(165)),
                );
            }
            if let Some(path) = &log_path {
                ui.small(
                    RichText::new(format!("Log: {}", path.display()))
                        .color(Color32::from_gray(165)),
                );
            }
        });

        ui.add_space(10.0);
        framed_panel(ui, |ui| {
            ui.label(RichText::new("Jobs").strong());
            ui.add_space(6.0);
            if let Some(active_job) = &self.active_job {
                ui.label(RichText::new(&active_job.label).strong());
                let fraction = if active_job.total == 0 {
                    0.0
                } else {
                    active_job.processed as f32 / active_job.total as f32
                };
                ui.add(ProgressBar::new(fraction.clamp(0.0, 1.0)).text(format!(
                    "{} / {} complete",
                    active_job.processed, active_job.total
                )));
                ui.small(format!(
                    "Success {}, failed {}, current {}",
                    active_job.succeeded,
                    active_job.failed,
                    if active_job.current_item.is_empty() {
                        "waiting...".to_owned()
                    } else {
                        truncate_for_ui(&active_job.current_item, 80)
                    }
                ));
                if ui.button("Cancel current job").clicked() {
                    self.cancel_active_job();
                }
            } else {
                ui.small(
                    RichText::new("No running import or upload job.")
                        .color(Color32::from_gray(150)),
                );
            }

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Queue: {}",
                    if self.queue_paused {
                        "Paused"
                    } else {
                        "Running"
                    }
                ));
                ui.label(format!("Queued jobs: {}", self.queued_jobs.len()));
                if ui
                    .add_enabled(!self.queue_paused, egui::Button::new("Pause queue"))
                    .clicked()
                {
                    self.pause_queue();
                }
                if ui
                    .add_enabled(self.queue_paused, egui::Button::new("Resume queue"))
                    .clicked()
                {
                    self.resume_queue();
                }
                if ui
                    .add_enabled(
                        self.active_job.is_none()
                            && self
                                .queued_jobs
                                .iter()
                                .any(|job| matches!(job.request, QueuedJobRequest::Upload { .. })),
                        egui::Button::new("Run queued upload now"),
                    )
                    .clicked()
                {
                    self.run_queued_upload_now();
                }
                if ui
                    .add_enabled(
                        !self.queued_jobs.is_empty(),
                        egui::Button::new("Clear queued jobs"),
                    )
                    .clicked()
                {
                    self.queued_jobs.clear();
                    self.persist_queue_state();
                    self.set_notice("Cleared queued jobs.", NoticeKind::Info);
                }
                if ui
                    .add_enabled(
                        !self.failed_fetches.is_empty(),
                        egui::Button::new("Retry failed imports"),
                    )
                    .clicked()
                {
                    self.retry_failed_fetches();
                }
                if ui
                    .add_enabled(
                        !self.last_failed_uploads.is_empty(),
                        egui::Button::new("Retry failed uploads"),
                    )
                    .clicked()
                {
                    self.retry_failed_uploads();
                }
            });

            if !self.queued_jobs.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Queue").strong());
                for job in self.queued_jobs.iter().take(6) {
                    ui.small(format!(
                        "#{} {} ({}){}",
                        job.id,
                        job.label,
                        job.kind.label(),
                        if self.queue_paused
                            && matches!(job.request, QueuedJobRequest::Upload { .. })
                        {
                            " [waiting]"
                        } else {
                            ""
                        }
                    ));
                }
            }

            if !self.completed_jobs.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Recent jobs").strong());
                for job in self.completed_jobs.iter().take(8) {
                    ui.small(format!(
                        "#{} {} ({}) [{}] {}",
                        job.id,
                        job.label,
                        job.kind.label(),
                        if job.success { "ok" } else { "issue" },
                        job.summary
                    ));
                }
            }
        });

        ui.add_space(10.0);
        framed_panel(ui, |ui| {
            ui.label(RichText::new("Recent log excerpt").strong());
            ui.add_space(6.0);
            match read_recent_log_excerpt(18) {
                Ok(text) => {
                    ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.code(text);
                    });
                }
                Err(err) => {
                    ui.small(RichText::new(err).color(Color32::from_rgb(238, 100, 100)));
                }
            }
        });
    }

    fn article_view(&mut self, ui: &mut egui::Ui) {
        let Some(article) = self.article_detail.clone() else {
            self.current_view = View::Library;
            return;
        };
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.current_view = View::Library;
                self.save_settings();
            }
            if ui.button("Copy Text").clicked() {
                ui.ctx().copy_text(article.clean_text.clone());
                self.set_notice("Article copied to clipboard.", NoticeKind::Success);
            }
            if ui.button("View original").clicked() {
                let _ = webbrowser::open(&article.url);
            }
        });
        ui.add_space(16.0);

        framed_panel(ui, |ui| {
            ui.heading(&article.title);
            if !article.subtitle.is_empty() {
                ui.label(RichText::new(&article.subtitle).italics().size(18.0));
            }
            ui.horizontal_wrapped(|ui| {
                if !article.author.is_empty() {
                    tag(ui, &format!("Von {}", article.author));
                }
                if !article.date.is_empty() {
                    tag(ui, &article.date);
                }
                tag(ui, &effective_topic_for_article(&article));
                tag(ui, &format!("{} words", article.word_count));
            });
            ui.separator();
            ScrollArea::vertical().show(ui, |ui| {
                for block in article.body_text.split("\n\n") {
                    if let Some(heading) = block.strip_prefix("## ") {
                        ui.add_space(8.0);
                        ui.label(RichText::new(heading).strong().size(22.0));
                    } else {
                        ui.label(block);
                    }
                    ui.add_space(10.0);
                }
            });
        });
    }

    fn preview_drawer(&mut self, ctx: &Context) {
        if !self.show_preview || self.current_view == View::Article {
            return;
        }

        let mut open_full_article = None;
        SidePanel::right("preview_drawer")
            .default_width(400.0)
            .min_width(320.0)
            .resizable(true)
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(18, 22, 30))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Preview");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.show_preview = false;
                        }
                    });
                });
                ui.separator();

                if self.preview_loading {
                    ui.spinner();
                    ui.label("Fetching article...");
                    return;
                }

                let Some(article) = self.preview_article.clone() else {
                    ui.label("No preview available.");
                    return;
                };

                ui.heading(&article.title);
                if !article.subtitle.is_empty() {
                    ui.label(
                        RichText::new(&article.subtitle)
                            .italics()
                            .color(Color32::from_gray(190)),
                    );
                }
                let preview_topic = self
                    .preview_stored_article
                    .as_ref()
                    .map(effective_topic_for_article)
                    .unwrap_or_else(|| {
                        generated_topic_from_fields(
                            &article.title,
                            &article.subtitle,
                            &article.section,
                            &article.url,
                        )
                    });
                ui.horizontal_wrapped(|ui| {
                    if !article.author.is_empty() {
                        tag(ui, &format!("Von {}", article.author));
                    }
                    if !article.date.is_empty() {
                        tag(ui, &article.date);
                    }
                    tag(ui, &preview_topic);
                    tag(ui, &format!("{} words", article.word_count));
                });
                ui.small(RichText::new(compact_url(&article.url)).color(Color32::from_gray(145)));

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(stored_article) = self.preview_stored_article.clone() {
                        if ui.button("Open full article").clicked() {
                            open_full_article = Some(stored_article);
                        }
                    }
                    if ui.link("Original").clicked() {
                        let _ = webbrowser::open(&article.url);
                    }
                    if ui.button("Copy text").clicked() {
                        ui.ctx().copy_text(article.clean_text.clone());
                        self.set_notice("Preview text copied to clipboard.", NoticeKind::Success);
                    }
                });

                ui.separator();
                ui.label(RichText::new("Quick preview").strong());
                ui.label(preview_excerpt(&article.body_text, 2, 900));

                ui.add_space(10.0);
                ui.collapsing("Show full extracted text", |ui| {
                    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for block in article.body_text.split("\n\n") {
                            if let Some(heading) = block.strip_prefix("## ") {
                                ui.add_space(6.0);
                                ui.label(RichText::new(heading).strong().size(18.0));
                            } else {
                                ui.label(block);
                            }
                            ui.add_space(6.0);
                        }
                    });
                });
            });
        if let Some(stored_article) = open_full_article {
            self.show_preview = false;
            self.open_article(stored_article);
        }
    }
}

impl eframe::App for SoziopolisLingqGui {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.guard_ui_phase("processing background events", |app| app.poll_events());
        ctx.request_repaint_after(Duration::from_millis(150));
        self.guard_ui_phase("rendering top notice", |app| app.top_notice(ctx));
        self.guard_ui_phase("rendering sidebar", |app| app.sidebar(ctx));
        self.guard_ui_phase("rendering LingQ settings", |app| {
            app.lingq_settings_window(ctx)
        });
        self.guard_ui_phase("rendering preview drawer", |app| app.preview_drawer(ctx));
        self.guard_ui_phase("rendering main view", |app| {
            egui::CentralPanel::default()
                .frame(
                    Frame::default()
                        .fill(Color32::from_rgb(15, 18, 25))
                        .inner_margin(Margin::same(20)),
                )
                .show(ctx, |ui| match app.current_view {
                    View::Browse => app.browse_view(ui),
                    View::Library => app.library_view(ui),
                    View::Article => app.article_view(ui),
                    View::Diagnostics => app.diagnostics_view(ui),
                });
        });
    }
}

fn render_library_article_card(
    app: &mut SoziopolisLingqGui,
    ui: &mut egui::Ui,
    article: StoredArticle,
) {
    article_card_frame(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                let mut selected_for_lingq = app.lingq_selected_articles.contains(&article.id);
                if ui.checkbox(&mut selected_for_lingq, "").changed() {
                    if selected_for_lingq {
                        app.lingq_selected_articles.insert(article.id);
                    } else {
                        app.lingq_selected_articles.remove(&article.id);
                    }
                }
                ui.label(RichText::new(&article.title).strong().size(14.5));
                if article.uploaded_to_lingq {
                    success_tag(ui, "Uploaded to LingQ");
                } else {
                    tag(ui, "Not uploaded to LingQ");
                }
            });
            let preview_line = library_card_preview_line(&article);
            if !preview_line.is_empty() {
                ui.small(
                    RichText::new(preview_line)
                        .color(Color32::from_gray(178))
                        .italics(),
                );
            }
            ui.horizontal_wrapped(|ui| {
                tag(ui, &effective_topic_for_article(&article));
                tag(ui, &format!("{} words", article.word_count));
                if !article.date.is_empty() {
                    tag(ui, &article.date);
                }
                if ui.button("Preview").clicked() {
                    app.open_library_preview(article.clone());
                }
                if ui.button("Open").clicked() {
                    app.open_article(article.clone());
                }
                if ui.link("Original").clicked() {
                    let _ = webbrowser::open(&article.url);
                }
                if ui.button("Delete").clicked() {
                    match Database::open_default().and_then(|db| db.delete_article(article.id)) {
                        Ok(_) => {
                            app.set_notice("Article deleted.", NoticeKind::Info);
                            app.refresh_after_content_change("article delete");
                        }
                        Err(err) => app.set_notice(err.to_string(), NoticeKind::Error),
                    }
                }
            });
            ui.small(RichText::new(compact_url(&article.url)).color(Color32::from_gray(155)));
        });
    });
}

fn truncate_for_ui(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        value.to_owned()
    } else {
        format!(
            "{}...",
            trim_chars_for_ui(value, max_chars.saturating_sub(3))
        )
    }
}

fn compact_url(url: &str) -> String {
    let cleaned = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    truncate_for_ui(cleaned, 72)
}

fn compare_library_articles(
    a: &StoredArticle,
    b: &StoredArticle,
    sort_mode: LibrarySortMode,
) -> std::cmp::Ordering {
    match sort_mode {
        LibrarySortMode::Newest => b.fetched_at.cmp(&a.fetched_at),
        LibrarySortMode::Oldest => a.fetched_at.cmp(&b.fetched_at),
        LibrarySortMode::Longest => b.word_count.cmp(&a.word_count),
        LibrarySortMode::Shortest => a.word_count.cmp(&b.word_count),
        LibrarySortMode::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
    }
}

fn trim_chars_for_ui(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

fn collect_topic_counts(articles: &[StoredArticle]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for article in articles {
        *counts
            .entry(effective_topic_for_article(article))
            .or_insert(0) += 1;
    }
    counts
}

fn article_matches_library_search(article: &StoredArticle, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }

    [
        article.title.as_str(),
        article.subtitle.as_str(),
        article.author.as_str(),
        article.section.as_str(),
        article.custom_topic.as_str(),
        article.url.as_str(),
    ]
    .iter()
    .any(|value| value.to_lowercase().contains(&needle))
}

fn auto_topic_for_article(article: &StoredArticle) -> String {
    generated_topic_from_fields(
        &article.title,
        &article.subtitle,
        &article.section,
        &article.url,
    )
}

fn effective_topic_for_article(article: &StoredArticle) -> String {
    if !article.custom_topic.trim().is_empty() {
        return article.custom_topic.trim().to_owned();
    }

    auto_topic_for_article(article)
}

fn library_card_preview_line(article: &StoredArticle) -> String {
    if !article.subtitle.trim().is_empty() {
        return truncate_for_ui(article.subtitle.trim(), 160);
    }

    let preview = preview_excerpt(&article.body_text, 1, 160);
    if preview.is_empty() {
        String::new()
    } else {
        preview
    }
}

fn preview_excerpt(body_text: &str, max_blocks: usize, max_chars: usize) -> String {
    let mut blocks = Vec::new();
    for block in body_text.split("\n\n") {
        let cleaned = clean_preview_block(block);
        if cleaned.is_empty() {
            continue;
        }
        blocks.push(cleaned);
        if blocks.len() >= max_blocks {
            break;
        }
    }

    truncate_for_ui(&blocks.join("\n\n"), max_chars)
}

fn clean_preview_block(block: &str) -> String {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    trimmed
        .strip_prefix("## ")
        .unwrap_or(trimmed)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn stored_article_to_preview_article(article: &StoredArticle) -> Article {
    Article {
        url: article.url.clone(),
        title: article.title.clone(),
        subtitle: article.subtitle.clone(),
        author: article.author.clone(),
        date: article.date.clone(),
        section: article.section.clone(),
        body_text: article.body_text.clone(),
        clean_text: article.clean_text.clone(),
        word_count: article.word_count as usize,
        fetched_at: article.fetched_at.clone(),
    }
}

fn latest_saved_article_date(articles: &[StoredArticle]) -> Option<NaiveDate> {
    articles
        .iter()
        .filter_map(|article| parse_article_date(&article.date))
        .max()
}

fn browse_article_is_recent_enough(
    article: &ArticleSummary,
    latest_saved_date: Option<NaiveDate>,
) -> bool {
    let Some(latest_saved_date) = latest_saved_date else {
        return true;
    };

    let Some(article_date) = parse_article_date(&article.date) else {
        return true;
    };

    article_date >= latest_saved_date
}

fn parse_article_date(value: &str) -> Option<NaiveDate> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    for format in ["%d.%m.%Y", "%Y-%m-%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, format) {
            return Some(date);
        }
    }

    trimmed
        .get(..10)
        .and_then(|prefix| NaiveDate::parse_from_str(prefix, "%Y-%m-%d").ok())
}

fn format_naive_date(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

fn create_support_bundle(app: &SoziopolisLingqGui) -> Result<PathBuf, String> {
    let bundles_dir = app_paths::support_bundles_dir().map_err(|err| err.to_string())?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "now".to_owned());
    let bundle_dir = bundles_dir.join(format!("support-bundle-{timestamp}"));
    fs::create_dir_all(&bundle_dir).map_err(|err| err.to_string())?;

    let settings_path = app_paths::settings_path().ok();
    let database_path = app_paths::database_path().ok();
    let queue_state_path = app_paths::queue_state_path().ok();
    let log_path = app_paths::app_log_path().ok();
    let exe_path = std::env::current_exe().ok();

    let mut summary = Vec::new();
    summary.push(format!("Soziopolis Reader {}", env!("CARGO_PKG_VERSION")));
    summary.push(String::new());
    if let Some(path) = exe_path {
        summary.push(format!("Executable: {}", path.display()));
    }
    if let Ok(path) = app_paths::data_dir() {
        summary.push(format!("Data directory: {}", path.display()));
    }
    if let Some(path) = &settings_path {
        summary.push(format!("Settings: {}", path.display()));
    }
    if let Some(path) = &database_path {
        summary.push(format!("Database: {}", path.display()));
    }
    if let Some(path) = &queue_state_path {
        summary.push(format!("Queue state: {}", path.display()));
    }
    if let Some(path) = &log_path {
        summary.push(format!("Log: {}", path.display()));
    }
    summary.push(String::new());
    summary.push(format!("Current view: {}", app.current_view.as_str()));
    summary.push(format!("Library articles: {}", app.library_articles.len()));
    summary.push(format!("Browse articles: {}", app.browse_articles.len()));
    summary.push(format!("Queued jobs: {}", app.queued_jobs.len()));
    summary.push(format!("Recent jobs: {}", app.completed_jobs.len()));
    summary.push(format!(
        "LingQ connected: {}",
        if app.lingq_connected { "yes" } else { "no" }
    ));
    if let Some(date) = latest_saved_article_date(&app.library_articles) {
        summary.push(format!(
            "Latest saved article date: {}",
            format_naive_date(date)
        ));
    }

    fs::write(bundle_dir.join("README.txt"), summary.join("\r\n"))
        .map_err(|err| err.to_string())?;

    let diagnostics = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "current_view": app.current_view.as_str(),
        "browse_scope_label": &app.browse_scope_label,
        "browse_only_new": app.browse_only_new,
        "browse_articles": app.browse_articles.len(),
        "library_articles": app.library_articles.len(),
        "queued_jobs": &app.queued_jobs,
        "recent_jobs": &app.completed_jobs,
        "failed_imports": &app.failed_fetches,
        "failed_uploads": &app.last_failed_uploads,
        "library_stats": app.library_stats.as_ref().map(|stats| serde_json::json!({
            "total_articles": stats.total_articles,
            "uploaded_articles": stats.uploaded_articles,
            "average_word_count": stats.average_word_count,
            "sections": stats.sections.iter().map(|section| serde_json::json!({
                "section": &section.section,
                "count": section.count,
            })).collect::<Vec<_>>(),
        })),
    });
    let diagnostics_text =
        serde_json::to_string_pretty(&diagnostics).map_err(|err| err.to_string())?;
    fs::write(bundle_dir.join("diagnostics.json"), diagnostics_text)
        .map_err(|err| err.to_string())?;

    for (source, target_name) in [
        (settings_path.as_ref(), "settings.json"),
        (queue_state_path.as_ref(), "queue_state.json"),
        (log_path.as_ref(), "soziopolis-reader.log"),
    ] {
        if let Some(source) = source {
            if source.exists() {
                let _ = fs::copy(source, bundle_dir.join(target_name));
            }
        }
    }

    if let Some(database_path) = database_path {
        if database_path.exists() {
            let _ = fs::copy(&database_path, bundle_dir.join("soziopolis_lingq_tool.db"));
        }
    }

    Ok(bundle_dir)
}

fn load_lingq_api_key_from_storage(settings: &mut SettingsStore) -> (String, Option<String>) {
    let legacy_api_key = settings.legacy_lingq_api_key();
    match credential_store::load_lingq_api_key() {
        Ok(Some(api_key)) => {
            let cleanup_notice = if legacy_api_key.is_some() {
                settings.clear_legacy_lingq_api_key().err().map(|err| {
                    format!(
                        "LingQ is using the token from Windows Credential Manager, but the old settings copy could not be cleared: {err}"
                    )
                })
            } else {
                None
            };
            (api_key, cleanup_notice)
        }
        Ok(None) => {
            let Some(legacy_api_key) = legacy_api_key else {
                return (String::new(), None);
            };

            match credential_store::save_lingq_api_key(&legacy_api_key) {
                Ok(()) => {
                    let notice = match settings.clear_legacy_lingq_api_key() {
                        Ok(()) => Some(
                            "Moved your existing LingQ token into Windows Credential Manager."
                                .to_owned(),
                        ),
                        Err(err) => Some(format!(
                            "Moved your LingQ token into Windows Credential Manager, but the old settings copy could not be cleared: {err}"
                        )),
                    };
                    (legacy_api_key, notice)
                }
                Err(err) => (
                    legacy_api_key,
                    Some(format!(
                        "Could not migrate the existing LingQ token into Windows Credential Manager yet, so the old settings copy is still being used for now: {err}"
                    )),
                ),
            }
        }
        Err(err) => {
            if let Some(legacy_api_key) = legacy_api_key {
                (
                    legacy_api_key,
                    Some(format!(
                        "Could not access Windows Credential Manager, so the old settings token is still being used for now: {err}"
                    )),
                )
            } else {
                (String::new(), None)
            }
        }
    }
}

fn parse_positive_usize_input(value: &str, label: &str) -> Result<usize, String> {
    let trimmed = value.trim();
    let parsed = trimmed
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a positive whole number."))?;
    if parsed == 0 {
        return Err(format!("{label} must be greater than zero."));
    }
    Ok(parsed)
}

fn parse_optional_positive_usize_input(value: &str, label: &str) -> Result<Option<usize>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    parse_positive_usize_input(trimmed, label).map(Some)
}

fn classify_error_message(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("date") {
        "missing date".to_owned()
    } else if lower.contains("network")
        || lower.contains("request failed")
        || lower.contains("non-success response")
    {
        "network".to_owned()
    } else if lower.contains("too little text")
        || lower.contains("could not extract article body")
        || lower.contains("selector")
    {
        "parse failure".to_owned()
    } else if lower.contains("403")
        || lower.contains("401")
        || lower.contains("paywall")
        || lower.contains("forbidden")
    {
        "access".to_owned()
    } else {
        "fetch error".to_owned()
    }
}

fn configure_theme(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(20, 24, 33);
    visuals.panel_fill = Color32::from_rgb(15, 18, 25);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(28, 33, 45);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(28, 33, 45);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 50, 68);
    visuals.widgets.active.bg_fill = Color32::from_rgb(56, 78, 122);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(225, 229, 238);
    visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
    visuals.selection.bg_fill = Color32::from_rgb(74, 108, 198);
    visuals.hyperlink_color = Color32::from_rgb(129, 165, 255);
    ctx.set_visuals(visuals);
}

fn framed_panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    Frame::default()
        .fill(Color32::from_rgb(24, 28, 37))
        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 56, 74)))
        .corner_radius(12.0)
        .inner_margin(Margin::same(16))
        .show(ui, add_contents);
}

fn article_card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let card_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(card_width, 0.0),
        Layout::top_down(Align::Min),
        |ui| {
            ui.set_min_width(card_width);
            Frame::default()
                .fill(Color32::from_rgb(24, 28, 37))
                .stroke(Stroke::new(1.0, Color32::from_rgb(46, 56, 74)))
                .corner_radius(10.0)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_min_width((card_width - 24.0).max(0.0));
                    add_contents(ui);
                });
        },
    );
}

fn render_import_progress(ui: &mut egui::Ui, progress: &ImportProgress) {
    let fraction = progress.total.map(|total| {
        if total == 0 {
            0.0
        } else {
            (progress.processed as f32 / total as f32).clamp(0.0, 1.0)
        }
    });

    let mut bar = ProgressBar::new(fraction.unwrap_or(0.0))
        .desired_width(f32::INFINITY)
        .text(match progress.total {
            Some(total) => format!(
                "{}: {} / {} processed",
                progress.phase, progress.processed, total
            ),
            None => format!("{}: {} items processed", progress.phase, progress.processed),
        });

    if fraction.is_none() {
        bar = bar.animate(true);
    }

    ui.add(bar);
    ui.horizontal_wrapped(|ui| {
        ui.small(format!(
            "Saved {}, skipped existing {}, skipped out of range {}, failed {}",
            progress.saved_count,
            progress.skipped_existing,
            progress.skipped_out_of_range,
            progress.failed_count
        ));
    });
    if !progress.current_item.is_empty() {
        ui.small(RichText::new(&progress.current_item).monospace());
    }
}

fn tag(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Color32::from_rgb(200, 206, 219))
            .background_color(Color32::from_rgb(33, 39, 52)),
    );
}

fn success_tag(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Color32::from_rgb(96, 220, 137))
            .background_color(Color32::from_rgb(27, 53, 40)),
    );
}

fn sidebar_stat_row(ui: &mut egui::Ui, label: &str, value: i64) {
    Frame::default()
        .fill(Color32::from_rgb(24, 28, 37))
        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 56, 74)))
        .corner_radius(10.0)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(Color32::from_gray(170)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(value.to_string()).strong());
                });
            });
        });
}

fn open_path_in_explorer(path: &std::path::Path) -> Result<(), String> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {}", path.display(), err))
}

fn open_log_in_notepad(path: &std::path::Path) -> Result<(), String> {
    Command::new("notepad")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {} in Notepad: {}", path.display(), err))
}

fn read_recent_log_excerpt(max_lines: usize) -> Result<String, String> {
    let path = app_paths::app_log_path().map_err(|err| err.to_string())?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|err| format!("Could not read {}: {}", path.display(), err))?;
    let lines = raw.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    let excerpt = lines[start..].join("\n");
    if excerpt.trim().is_empty() {
        Ok("Log is currently empty.".to_owned())
    } else {
        Ok(excerpt)
    }
}

fn build_content_refresh_event(request_id: u64, reason: String) -> AppEvent {
    let result = match Database::open_default() {
        Ok(db) => {
            logging::info(format!(
                "content refresh {request_id}: loading imported URL cache"
            ));
            let imported_urls = db.get_all_article_urls().map_err(|err| err.to_string());

            logging::info(format!(
                "content refresh {request_id}: loading library articles"
            ));
            let library_articles = db
                .list_articles(None, None, false, 0)
                .map_err(|err| err.to_string());

            logging::info(format!(
                "content refresh {request_id}: loading library stats"
            ));
            let library_stats = db.get_stats().map_err(|err| err.to_string());

            ContentRefreshResult {
                imported_urls,
                library_articles,
                library_stats,
            }
        }
        Err(err) => {
            let message = err.to_string();
            logging::error(format!(
                "content refresh {request_id}: failed to open database: {message}"
            ));
            ContentRefreshResult {
                imported_urls: Err(message.clone()),
                library_articles: Err(message.clone()),
                library_stats: Err(message),
            }
        }
    };

    AppEvent::ContentRefreshCompleted {
        request_id,
        reason,
        result,
    }
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
