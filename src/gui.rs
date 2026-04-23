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
mod views;

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

impl SoziopolisLingqGui {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let (settings, settings_notice) = match SettingsStore::load_default() {
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
        let (lingq_api_key, startup_notice) = load_lingq_api_key_from_storage();
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

    fn persist_lingq_api_key(&mut self) -> bool {
        let api_key = self.lingq_api_key.trim().to_owned();
        if api_key.is_empty() {
            self.set_notice("Enter a LingQ token first.", NoticeKind::Error);
            return false;
        }

        match credential_store::save_lingq_api_key(&api_key) {
            Ok(()) => {
                self.lingq_api_key = api_key;
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

    fn next_job_id(&mut self) -> u64 {
        self.next_job_id = self.next_job_id.wrapping_add(1);
        self.next_job_id
    }

    fn load_persisted_queue_state(&mut self) {
        let mut database = match Database::open_default() {
            Ok(database) => database,
            Err(err) => {
                logging::warn(format!("could not open SQLite for queue restore: {err}"));
                return;
            }
        };
        let repository = JobRepository::new(&mut database);
        match repository.load_snapshot() {
            Ok(snapshot) => {
                self.next_job_id = self.next_job_id.max(snapshot.next_job_id);
                self.queue_paused = snapshot.queue_paused;
                self.queued_jobs = snapshot.queued_jobs.into();
                self.completed_jobs = snapshot.completed_jobs.into();
                self.failed_fetches = snapshot.failed_fetches;
                self.last_failed_uploads = snapshot.failed_uploads;
                if let Ok(history) = repository.list_completed_job_history(10) {
                    self.completed_jobs = history.into();
                }
                if !self.queued_jobs.is_empty() {
                    logging::info(format!(
                        "restored {} queued job(s) from SQLite",
                        self.queued_jobs.len()
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
            Err(err) => logging::warn(format!("could not load queue state from SQLite: {err}")),
        }
    }

    fn persist_queue_state(&self) {
        let snapshot = QueueSnapshot {
            next_job_id: self.next_job_id,
            queue_paused: self.queue_paused,
            queued_jobs: self.queued_jobs.iter().cloned().collect(),
            completed_jobs: self.completed_jobs.iter().cloned().collect(),
            failed_fetches: self.failed_fetches.clone(),
            failed_uploads: self.last_failed_uploads.clone(),
        };

        let mut database = match Database::open_default() {
            Ok(database) => database,
            Err(err) => {
                logging::warn(format!(
                    "could not open SQLite for queue persistence: {err}"
                ));
                return;
            }
        };
        let mut repository = JobRepository::new(&mut database);
        if let Err(err) = repository.save_snapshot(&snapshot) {
            logging::warn(format!("could not persist queue state to SQLite: {err}"));
        }
    }

    fn enqueue_import_job(&mut self, articles: Vec<ArticleSummary>) {
        if articles.is_empty() {
            self.set_notice("Select at least one article first.", NoticeKind::Error);
            return;
        }
        let total = articles.len();
        let job = QueuedJob {
            id: self.next_job_id(),
            kind: JobKind::Import,
            label: format!("Import {} article(s)", total),
            total,
            request: QueuedJobRequest::Import { articles },
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
            QueuedJobRequest::Import { articles } => {
                self.spawn_import_job(job.id, articles, cancel_flag)
            }
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
        let completed_job = CompletedJob {
            id,
            kind,
            label,
            summary,
            success,
            recorded_at: job_timestamp_now(),
        };
        self.completed_jobs.push_front(completed_job.clone());
        while self.completed_jobs.len() > 10 {
            self.completed_jobs.pop_back();
        }
        if let Ok(mut database) = Database::open_default() {
            let repository = JobRepository::new(&mut database);
            if let Err(err) = repository.record_completed_job_history(&completed_job) {
                logging::warn(format!("could not persist completed job history: {err}"));
            }
        }
        self.persist_queue_state();
    }

    fn batch_fetch_selected(&mut self) {
        let selected_urls = self.browse_selected.iter().cloned().collect::<HashSet<_>>();
        let mut articles = self
            .browse_articles
            .iter()
            .filter(|article| selected_urls.contains(&article.url))
            .cloned()
            .collect::<Vec<_>>();
        for url in selected_urls {
            if articles.iter().any(|article| article.url == url) {
                continue;
            }
            articles.push(ArticleSummary {
                url,
                title: String::new(),
                teaser: String::new(),
                author: String::new(),
                date: String::new(),
                section: String::new(),
                source_kind: crate::soziopolis::DiscoverySourceKind::Section,
                source_label: String::new(),
            });
        }
        self.enqueue_import_job(articles);
    }

    fn spawn_import_job(
        &self,
        job_id: u64,
        articles: Vec<ArticleSummary>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        logging::info(format!("starting import worker for job #{job_id}"));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match panic::catch_unwind(AssertUnwindSafe(|| {
                let progress_tx = tx.clone();
                let result =
                    BrowseService::import_articles(articles, cancel_flag, move |progress| {
                        let _ = progress_tx.send(AppEvent::BatchFetchProgress(progress));
                    })
                    .map_err(|err| err.to_string());
                match result {
                    Ok(result) => AppEvent::BatchFetched {
                        job_id,
                        saved_count: result.saved_count,
                        skipped_existing: result.skipped_existing,
                        skipped_out_of_range: result.skipped_out_of_range,
                        failed: result.failed,
                        canceled: result.canceled,
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
                let progress_tx = tx.clone();
                let result = LingqService::upload_articles(
                    ids,
                    api_key,
                    collection_id,
                    cancel_flag,
                    move |progress| {
                        let _ = progress_tx.send(AppEvent::UploadProgress { job_id, progress });
                    },
                )
                .map_err(|err| err.to_string());
                match result {
                    Ok(result) => AppEvent::BatchUploaded {
                        job_id,
                        uploaded: result.uploaded,
                        failed: result.failed,
                        canceled: result.canceled,
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
        let articles = self
            .failed_fetches
            .iter()
            .map(|item| ArticleSummary {
                url: item.url.clone(),
                title: item.title.clone(),
                teaser: String::new(),
                author: String::new(),
                date: String::new(),
                section: String::new(),
                source_kind: crate::soziopolis::DiscoverySourceKind::Section,
                source_label: String::new(),
            })
            .collect::<Vec<_>>();
        self.enqueue_import_job(articles);
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
        !self.browse_imported_urls.contains(&article.url)
    }

    fn browse_article_is_visible(&self, article: &ArticleSummary) -> bool {
        let passes_new = !self.browse_only_new || self.browse_article_passes_new_filter(article);
        let search = self.browse_search.trim().to_lowercase();
        let passes_search = if search.is_empty() {
            true
        } else {
            [
                article.title.as_str(),
                article.teaser.as_str(),
                article.author.as_str(),
                article.date.as_str(),
                article.section.as_str(),
                article.url.as_str(),
            ]
            .iter()
            .any(|field| field.to_lowercase().contains(&search))
        };

        passes_new && passes_search
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

        let mut articles = if self.library_search.trim().is_empty() {
            self.library_articles.clone()
        } else {
            let database = Database::open_default().map_err(|err| err.to_string())?;
            let repository = ArticleRepository::new(&database);
            repository
                .list_articles(Some(&self.library_search), None, false, 0)
                .map_err(|err| err.to_string())?
        };

        articles = articles
            .into_iter()
            .filter(|article| {
                (self.library_topic.trim().is_empty()
                    || effective_topic_for_article(article) == self.library_topic)
                    && (!self.library_only_not_uploaded || !article.uploaded_to_lingq)
                    && min_words.is_none_or(|min| article.word_count as usize >= min)
                    && max_words.is_none_or(|max| article.word_count as usize <= max)
            })
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

    fn sidebar(&mut self, ctx: &Context) {
        self.render_sidebar(ctx);
    }
    fn top_notice(&mut self, ctx: &Context) {
        self.render_top_notice(ctx);
    }
    fn lingq_settings_window(&mut self, ctx: &Context) {
        self.render_lingq_settings_window(ctx);
    }
    fn browse_view(&mut self, ui: &mut egui::Ui) {
        self.render_browse_view(ui);
    }
    fn library_view(&mut self, ui: &mut egui::Ui) {
        self.render_library_view(ui);
    }
    fn lingq_panel(&mut self, ui: &mut egui::Ui) {
        self.render_lingq_panel(ui);
    }
    fn diagnostics_view(&mut self, ui: &mut egui::Ui) {
        self.render_diagnostics_view(ui);
    }
    fn article_view(&mut self, ui: &mut egui::Ui) {
        self.render_article_view(ui);
    }
    fn preview_drawer(&mut self, ctx: &Context) {
        self.render_preview_drawer(ctx);
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
                    match LibraryService::delete_article(article.id) {
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
        LibrarySortMode::Newest => b
            .published_at
            .cmp(&a.published_at)
            .then_with(|| b.fetched_at.cmp(&a.fetched_at)),
        LibrarySortMode::Oldest => a
            .published_at
            .cmp(&b.published_at)
            .then_with(|| a.fetched_at.cmp(&b.fetched_at)),
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
    if !article.teaser.trim().is_empty() {
        return truncate_for_ui(article.teaser.trim(), 160);
    }

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
        teaser: article.teaser.clone(),
        author: article.author.clone(),
        date: article.date.clone(),
        published_at: article.published_at.clone(),
        section: article.section.clone(),
        source_kind: article.source_kind.clone(),
        source_label: article.source_label.clone(),
        body_text: article.body_text.clone(),
        clean_text: article.clean_text.clone(),
        word_count: article.word_count as usize,
        fetched_at: article.fetched_at.clone(),
    }
}

fn latest_saved_article_date(articles: &[StoredArticle]) -> Option<NaiveDate> {
    articles
        .iter()
        .filter_map(|article| {
            if !article.published_at.trim().is_empty() {
                parse_article_date(&article.published_at)
            } else {
                parse_article_date(&article.date)
            }
        })
        .max()
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

    let queue_snapshot = QueueSnapshot {
        next_job_id: app.next_job_id,
        queue_paused: app.queue_paused,
        queued_jobs: app.queued_jobs.iter().cloned().collect(),
        completed_jobs: app.completed_jobs.iter().cloned().collect(),
        failed_fetches: app.failed_fetches.clone(),
        failed_uploads: app.last_failed_uploads.clone(),
    };
    let queue_snapshot_text =
        serde_json::to_string_pretty(&queue_snapshot).map_err(|err| err.to_string())?;
    fs::write(bundle_dir.join("queue_snapshot.json"), queue_snapshot_text)
        .map_err(|err| err.to_string())?;

    if let Some(source) = settings_path.as_ref() {
        if source.exists() {
            match fs::read_to_string(source) {
                Ok(raw) => {
                    let redacted =
                        raw.replace("\"lingq_api_key\":", "\"legacy_lingq_api_key_removed\":");
                    let _ = fs::write(bundle_dir.join("settings.json"), redacted);
                }
                Err(_) => {
                    let _ = fs::copy(source, bundle_dir.join("settings.json"));
                }
            }
        }
    }

    if let Some(source) = log_path.as_ref() {
        if source.exists() {
            let _ = fs::copy(source, bundle_dir.join("soziopolis-reader.log"));
        }
    }

    if let Some(database_path) = database_path {
        if database_path.exists() {
            let _ = fs::copy(&database_path, bundle_dir.join("soziopolis_lingq_tool.db"));
        }
    }

    Ok(bundle_dir)
}

fn load_lingq_api_key_from_storage() -> (String, Option<String>) {
    match credential_store::load_lingq_api_key() {
        Ok(Some(api_key)) => (api_key, None),
        Ok(None) => (String::new(), None),
        Err(err) => (
            String::new(),
            Some(format!(
                "Could not access Windows Credential Manager, so LingQ remains disconnected: {err}"
            )),
        ),
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

fn job_timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

fn parse_optional_positive_usize_input(value: &str, label: &str) -> Result<Option<usize>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    parse_positive_usize_input(trimmed, label).map(Some)
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
        ui.small(format_import_progress_details(progress));
    });
    if !progress.current_item.is_empty() {
        ui.small(RichText::new(&progress.current_item).monospace());
    }
}

fn format_import_progress_details(progress: &ImportProgress) -> String {
    let mut parts = vec![format!("Saved {}", progress.saved_count)];
    if progress.skipped_existing > 0 {
        parts.push(format!("skipped existing {}", progress.skipped_existing));
    }
    if progress.skipped_out_of_range > 0 {
        parts.push(format!(
            "skipped out of range {}",
            progress.skipped_out_of_range
        ));
    }
    parts.push(format!("failed {}", progress.failed_count));
    parts.join(", ")
}

fn format_import_result_summary(
    saved_count: usize,
    skipped_existing: usize,
    skipped_out_of_range: usize,
    failed_count: usize,
    canceled: bool,
    first_error: Option<&str>,
) -> String {
    let mut segments = Vec::new();
    if canceled {
        segments.push("Import canceled.".to_owned());
    }

    segments.push(format!("Saved {saved_count} article(s)."));

    let mut detail_parts = Vec::new();
    if skipped_existing > 0 {
        detail_parts.push(format!("skipped {skipped_existing} already imported"));
    }
    if skipped_out_of_range > 0 {
        detail_parts.push(format!(
            "skipped {skipped_out_of_range} outside the date window"
        ));
    }
    if failed_count > 0 {
        detail_parts.push(format!("{failed_count} failed"));
    }

    if !detail_parts.is_empty() {
        segments.push(format!("Also {}.", detail_parts.join(", ")));
    }

    if let Some(first_error) = first_error {
        segments.push(format!("First error: {first_error}"));
    }

    segments.join(" ")
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
    logging::info(format!(
        "content refresh {request_id}: loading imported URL cache, library articles, and stats"
    ));
    let result = LibraryService::refresh_content();

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
