use super::*;
use std::time::{Duration, Instant};

const PROGRESS_EVENT_MIN_INTERVAL: Duration = Duration::from_millis(125);

fn should_emit_progress_update(
    last_emitted_at: &mut Option<Instant>,
    processed: usize,
    total: Option<usize>,
) -> bool {
    if processed == 0 {
        return true;
    }
    if total.is_some_and(|total| processed >= total) {
        *last_emitted_at = Some(Instant::now());
        return true;
    }
    match last_emitted_at {
        Some(last_emitted) if last_emitted.elapsed() < PROGRESS_EVENT_MIN_INTERVAL => false,
        _ => {
            *last_emitted_at = Some(Instant::now());
            true
        }
    }
}

impl SoziopolisLingqGui {
    pub(super) fn next_job_id(&mut self) -> u64 {
        self.next_job_id = self.next_job_id.wrapping_add(1);
        self.next_job_id
    }

    pub(super) fn load_persisted_queue_state(&mut self) {
        let shared_db = match Database::shared_default() {
            Ok(database) => database,
            Err(err) => {
                logging::warn(format!("could not open SQLite for queue restore: {err}"));
                return;
            }
        };
        match shared_db.with_db(|database| {
            let repository = JobRepository::new(database);
            let snapshot = repository.load_snapshot()?;
            let history = repository.list_completed_job_history(25).ok();
            Ok((snapshot, history))
        }) {
            Ok((snapshot, history)) => {
                self.next_job_id = self.next_job_id.max(snapshot.next_job_id);
                self.queue_paused = snapshot.queue_paused;
                self.queued_jobs = snapshot.queued_jobs.into();
                self.completed_jobs = snapshot.completed_jobs.into();
                self.failed_fetches = snapshot.failed_fetches;
                self.last_failed_uploads = snapshot.failed_uploads;
                if let Some(history) = history {
                    self.completed_jobs = history.into();
                }
                self.diagnostics_selected_job_id = self.completed_jobs.front().map(|job| job.id);
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

    pub(super) fn persist_queue_state(&self) {
        let snapshot = QueueSnapshot {
            next_job_id: self.next_job_id,
            queue_paused: self.queue_paused,
            queued_jobs: self.queued_jobs.iter().cloned().collect(),
            completed_jobs: self.completed_jobs.iter().cloned().collect(),
            failed_fetches: self.failed_fetches.clone(),
            failed_uploads: self.last_failed_uploads.clone(),
        };

        let shared_db = match Database::shared_default() {
            Ok(database) => database,
            Err(err) => {
                logging::warn(format!(
                    "could not open SQLite for queue persistence: {err}"
                ));
                return;
            }
        };
        if let Err(err) = shared_db.with_db(|database| {
            let mut repository = JobRepository::new(database);
            repository.save_snapshot(&snapshot)
        }) {
            logging::warn(format!("could not persist queue state to SQLite: {err}"));
        }
    }

    pub(super) fn enqueue_import_job(&mut self, articles: Vec<ArticleSummary>) {
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

    pub(super) fn enqueue_upload_job(&mut self, ids: Vec<i64>, collection_id: Option<i64>) {
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

    pub(super) fn enqueue_job(&mut self, job: QueuedJob) {
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

    pub(super) fn can_start_job(&mut self, job: &QueuedJob) -> bool {
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

    pub(super) fn start_next_queued_job(&mut self) {
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

    pub(super) fn start_job(&mut self, job: QueuedJob) {
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

    pub(super) fn cancel_active_job(&mut self) {
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

    pub(super) fn pause_queue(&mut self) {
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

    pub(super) fn resume_queue(&mut self) {
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

    pub(super) fn run_queued_upload_now(&mut self) {
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

    pub(super) fn record_completed_job(
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
        self.diagnostics_selected_job_id = Some(completed_job.id);
        while self.completed_jobs.len() > 25 {
            self.completed_jobs.pop_back();
        }
        if let Ok(shared_db) = Database::shared_default() {
            if let Err(err) = shared_db.with_db(|database| {
                let repository = JobRepository::new(database);
                repository.record_completed_job_history(&completed_job)
            }) {
                logging::warn(format!("could not persist completed job history: {err}"));
            }
        }
        self.persist_queue_state();
    }

    pub(super) fn batch_fetch_selected(&mut self) {
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

    pub(super) fn spawn_import_job(
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
                let mut last_progress_emit = None;
                let result =
                    BrowseService::import_articles(articles, cancel_flag, move |progress| {
                        let total = progress.total;
                        if should_emit_progress_update(
                            &mut last_progress_emit,
                            progress.processed,
                            total,
                        ) {
                            let _ = progress_tx.send(AppEvent::BatchFetchProgress(progress));
                        }
                    })
                    .map_err(|err| err.to_string());
                match result {
                    Ok(result) => AppEvent::BatchFetched {
                        job_id,
                        saved_count: result.saved_count,
                        saved_articles: result.saved_articles,
                        skipped_existing: result.skipped_existing,
                        skipped_out_of_range: result.skipped_out_of_range,
                        failed: result.failed,
                        canceled: result.canceled,
                    },
                    Err(err) => AppEvent::BatchFetched {
                        job_id,
                        saved_count: 0,
                        saved_articles: Vec::new(),
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
                        saved_articles: Vec::new(),
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

    pub(super) fn spawn_upload_job(
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
                let mut last_progress_emit = None;
                let result = LingqService::upload_articles(
                    ids,
                    api_key,
                    collection_id,
                    cancel_flag,
                    move |progress| {
                        if should_emit_progress_update(
                            &mut last_progress_emit,
                            progress.processed,
                            Some(progress.total),
                        ) {
                            let _ = progress_tx.send(AppEvent::UploadProgress { job_id, progress });
                        }
                    },
                )
                .map_err(|err| err.to_string());
                match result {
                    Ok(result) => AppEvent::BatchUploaded {
                        job_id,
                        uploaded: result.uploaded,
                        successes: result.successes,
                        failed: result.failed,
                        canceled: result.canceled,
                    },
                    Err(err) => AppEvent::BatchUploaded {
                        job_id,
                        uploaded: 0,
                        successes: Vec::new(),
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
                        successes: Vec::new(),
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

    pub(super) fn retry_failed_fetches(&mut self) {
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

    pub(super) fn retry_failed_uploads(&mut self) {
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

    pub(super) fn select_lingq_articles_by_word_count(&mut self) {
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
}
