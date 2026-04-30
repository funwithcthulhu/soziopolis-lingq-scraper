use super::*;

/// Yield control back to the async executor without depending on tokio.
async fn async_std_yield() {
    struct Yield(bool);
    impl std::future::Future for Yield {
        type Output = ();
        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<()> {
            if self.0 {
                std::task::Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }
    }
    Yield(false).await;
}

/// Run a blocking closure on a background thread, returning a future.
pub(super) async fn run_blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    // Poll in a yielding loop — iced runs this on its async executor.
    loop {
        match rx.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("blocking task thread panicked");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                async_std_yield().await;
            }
        }
    }
}

impl App {
    pub(super) fn app_context(&self) -> Result<AppContext, String> {
        self.app_context.clone().ok_or_else(|| {
            self.app_context_error
                .clone()
                .unwrap_or_else(|| "The app database is unavailable right now.".to_owned())
        })
    }

    pub(super) fn save_settings(&mut self) {
        let current_view = self.current_view;
        let browse_section = self.browse_section.clone();
        let browse_only_new = self.browse_only_new;
        let lingq_collection_id = self.lingq_selected_collection;
        if let Err(err) = self.settings.update(|s| {
            s.last_view = current_view.as_str().to_owned();
            s.browse_section = browse_section;
            s.browse_only_new = browse_only_new;
            s.lingq_collection_id = lingq_collection_id;
        }) {
            self.set_notice(
                format!("Could not save app settings: {err}"),
                NoticeKind::Error,
            );
        }
    }

    pub(super) fn set_notice(&mut self, message: impl Into<String>, kind: NoticeKind) {
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

    pub(super) fn record_task_failure(&mut self, error: AppError) {
        if self.recent_task_failures.len() >= 20 {
            self.recent_task_failures.pop_back();
        }
        self.recent_task_failures.push_front(error);
    }

    pub(super) fn set_task_error_notice(&mut self, error: AppError) {
        let notice = error.notice_message();
        self.record_task_failure(error);
        self.set_notice(notice, NoticeKind::Error);
    }

    // ── Browse task spawning ────────────────────────────────────

    pub(super) fn spawn_browse_refresh(&mut self) -> Task<Message> {
        self.browse_scope = BrowseScope::CurrentSection;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_end_reached = false;
        self.browse_session_state = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let section = self.browse_section.clone();
        let limit = self.browse_limit;
        Task::perform(
            run_blocking(move || {
                BrowseService::browse_section(&section, limit)
                    .map_err(|err| AppError::classify("browse section", err.to_string()))
            }),
            move |result| Message::BrowseLoaded { request_id, result },
        )
    }

    pub(super) fn spawn_browse_all_sections(&mut self) -> Task<Message> {
        self.browse_scope = BrowseScope::AllSections;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_end_reached = false;
        self.browse_session_state = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let limit = self.browse_limit;
        Task::perform(
            run_blocking(move || {
                BrowseService::browse_all_sections(limit)
                    .map_err(|err| AppError::classify("browse all sections", err.to_string()))
            }),
            move |result| Message::BrowseLoaded { request_id, result },
        )
    }

    pub(super) fn spawn_load_more_current_section(&mut self) -> Task<Message> {
        if self.browse_loading {
            return Task::none();
        }
        self.browse_scope = BrowseScope::CurrentSection;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let limit = self.browse_limit;

        match self.browse_session_state.clone() {
            Some(BrowseSessionState::CurrentSection(state)) => Task::perform(
                run_blocking(move || {
                    BrowseService::continue_browse_section(state, limit)
                        .map_err(|err| AppError::classify("continue browse", err.to_string()))
                }),
                move |result| Message::BrowseLoaded { request_id, result },
            ),
            _ => self.spawn_browse_refresh(),
        }
    }

    pub(super) fn spawn_load_more_all_sections(&mut self) -> Task<Message> {
        if self.browse_loading {
            return Task::none();
        }
        self.browse_scope = BrowseScope::AllSections;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let limit = self.browse_limit;

        match self.browse_session_state.clone() {
            Some(BrowseSessionState::AllSections(state)) => Task::perform(
                run_blocking(move || {
                    BrowseService::continue_browse_all_sections(state, limit)
                        .map_err(|err| AppError::classify("continue browse all", err.to_string()))
                }),
                move |result| Message::BrowseLoaded { request_id, result },
            ),
            _ => self.spawn_browse_all_sections(),
        }
    }

    // ── Content refresh ─────────────────────────────────────────

    pub(super) fn spawn_content_refresh(&mut self, reason: &str) -> Task<Message> {
        self.library_loading = true;
        self.content_refresh_request_id = self.content_refresh_request_id.wrapping_add(1);
        let request_id = self.content_refresh_request_id;
        let reason = reason.to_owned();
        let app_context = self.app_context().ok();
        Task::perform(
            run_blocking(move || build_content_refresh_event(app_context, request_id, reason)),
            |event| event,
        )
    }

    // ── Preview ─────────────────────────────────────────────────

    pub(super) fn spawn_open_preview(&mut self, url: String) -> Task<Message> {
        self.preview_loading = true;
        self.show_preview = true;
        self.preview_article = None;
        self.preview_stored_article = None;
        let app_context = self.app_context().ok();
        Task::perform(
            run_blocking(move || {
                let article = BrowseService::preview_article(&url)
                    .map_err(|err| AppError::classify("preview article", err.to_string()))?;
                let stored = app_context.and_then(|ctx| {
                    ctx.db
                        .with_db(|db| {
                            let repo = ArticleRepository::new(db);
                            repo.get_articles_by_urls(&[article.url.as_str()])
                        })
                        .ok()
                        .and_then(|mut v| {
                            if v.is_empty() {
                                None
                            } else {
                                Some(v.remove(0))
                            }
                        })
                });
                Ok((article, stored))
            }),
            Message::PreviewLoaded,
        )
    }

    // ── LingQ ───────────────────────────────────────────────────

    pub(super) fn spawn_load_collections(&mut self) -> Task<Message> {
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Enter a LingQ API key first.", NoticeKind::Error);
            return Task::none();
        }
        self.lingq_loading_collections = true;
        let api_key = self.lingq_api_key.clone();
        Task::perform(
            run_blocking(move || {
                LingqService::collections(&api_key, "de")
                    .map_err(|err| AppError::classify("load LingQ courses", err.to_string()))
            }),
            Message::CollectionsLoaded,
        )
    }

    pub(super) fn spawn_login_to_lingq(&mut self) -> Task<Message> {
        if self.lingq_username.trim().is_empty() || self.lingq_password.is_empty() {
            self.set_notice("Enter your LingQ username and password.", NoticeKind::Error);
            return Task::none();
        }
        self.lingq_loading_collections = true;
        let username = self.lingq_username.clone();
        let password = self.lingq_password.clone();
        self.lingq_password.clear();
        Task::perform(
            run_blocking(move || {
                LingqService::login(&username, &password)
                    .map_err(|err| AppError::classify("LingQ login", err.to_string()))
            }),
            Message::LingqLoggedIn,
        )
    }

    // ── Import / Upload job spawning ────────────────────────────

    pub(super) fn spawn_import_job(
        &mut self,
        job_id: u64,
        articles: Vec<ArticleSummary>,
    ) -> Task<Message> {
        let app_context = self.app_context().ok();
        let cancel_flag = self
            .active_job
            .as_ref()
            .map(|j| j.cancel_flag.clone())
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        Task::perform(
            run_blocking(move || {
                let result = app_context
                    .ok_or_else(|| anyhow::anyhow!("Database unavailable"))
                    .and_then(|ctx| {
                        BrowseService::import_articles(
                            &ctx,
                            articles,
                            cancel_flag,
                            |_progress| { /* progress via subscription not needed for now */ },
                        )
                    })
                    .map_err(|err| err.to_string());
                match result {
                    Ok(r) => Message::BatchFetched {
                        job_id,
                        saved_count: r.saved_count,
                        saved_articles: r.saved_articles,
                        skipped_existing: r.skipped_existing,
                        skipped_out_of_range: r.skipped_out_of_range,
                        failed: r.failed,
                        canceled: r.canceled,
                    },
                    Err(err) => Message::BatchFetched {
                        job_id,
                        saved_count: 0,
                        saved_articles: Vec::new(),
                        skipped_existing: 0,
                        skipped_out_of_range: 0,
                        failed: vec![FailedFetchItem {
                            url: String::new(),
                            title: String::new(),
                            category: "error".to_owned(),
                            message: err,
                        }],
                        canceled: false,
                    },
                }
            }),
            |msg| msg,
        )
    }

    pub(super) fn spawn_upload_job(
        &mut self,
        job_id: u64,
        ids: Vec<i64>,
        api_key: String,
        collection_id: Option<i64>,
    ) -> Task<Message> {
        let app_context = self.app_context().ok();
        let cancel_flag = self
            .active_job
            .as_ref()
            .map(|j| j.cancel_flag.clone())
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        Task::perform(
            run_blocking(move || {
                let result = app_context
                    .ok_or_else(|| anyhow::anyhow!("Database unavailable"))
                    .and_then(|ctx| {
                        LingqService::upload_articles(
                            &ctx,
                            ids,
                            api_key,
                            collection_id,
                            cancel_flag,
                            |_progress| {},
                        )
                    })
                    .map_err(|err| err.to_string());
                match result {
                    Ok(r) => Message::BatchUploaded {
                        job_id,
                        uploaded: r.uploaded,
                        successes: r.successes,
                        failed: r.failed,
                        canceled: r.canceled,
                    },
                    Err(err) => Message::BatchUploaded {
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
            }),
            |msg| msg,
        )
    }

    // ── Job queue management ────────────────────────────────────

    pub(super) fn next_job_id(&mut self) -> u64 {
        self.next_job_id = self.next_job_id.wrapping_add(1);
        self.next_job_id
    }

    pub(super) fn load_persisted_queue_state(&mut self) {
        let app_context = match self.app_context() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        if let Ok((snapshot, history)) = app_context.db.with_db(|database| {
            let repository = JobRepository::new(database);
            let snapshot = repository.load_snapshot()?;
            let history = repository.list_completed_job_history(25).ok();
            Ok((snapshot, history))
        }) {
            self.next_job_id = self.next_job_id.max(snapshot.next_job_id);
            self.queue_paused = snapshot.queue_paused;
            self.queued_jobs = snapshot.queued_jobs.into();
            self.completed_jobs = snapshot.completed_jobs.into();
            self.failed_fetches = snapshot.failed_fetches;
            self.last_failed_uploads = snapshot.failed_uploads;
            if let Some(history) = history {
                self.completed_jobs = history.into();
            }
            self.diagnostics_selected_job_id = self.completed_jobs.front().map(|j| j.id);
            if !self.queued_jobs.is_empty() {
                self.set_notice(
                    format!("Restored {} queued job(s).", self.queued_jobs.len()),
                    NoticeKind::Info,
                );
            }
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
        if let Ok(ctx) = self.app_context() {
            let _ = ctx.db.with_db(|database| {
                let mut repository = JobRepository::new(database);
                repository.save_snapshot(&snapshot)
            });
        }
    }

    pub(super) fn enqueue_job(&mut self, job: QueuedJob) -> Task<Message> {
        if self.active_job.is_some() {
            self.queued_jobs.push_back(job);
            self.persist_queue_state();
            self.set_notice(
                format!("Job queued. {} job(s) waiting.", self.queued_jobs.len()),
                NoticeKind::Info,
            );
            return Task::none();
        }
        let task = self.start_job(job);
        self.persist_queue_state();
        task
    }

    pub(super) fn start_job(&mut self, job: QueuedJob) -> Task<Message> {
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

        match &job.request {
            QueuedJobRequest::Import { articles } => {
                self.failed_fetches.clear();
                self.spawn_import_job(job.id, articles.clone())
            }
            QueuedJobRequest::Upload { ids, collection_id } => self.spawn_upload_job(
                job.id,
                ids.clone(),
                self.lingq_api_key.clone(),
                *collection_id,
            ),
        }
    }

    pub(super) fn try_start_next_queued_job(&mut self) -> Task<Message> {
        if self.active_job.is_some() || self.queue_paused {
            return Task::none();
        }
        if let Some(job) = self.queued_jobs.pop_front() {
            if matches!(job.request, QueuedJobRequest::Upload { .. })
                && self.lingq_api_key.trim().is_empty()
            {
                self.set_notice(
                    "Queued uploads waiting for LingQ connection.",
                    NoticeKind::Info,
                );
                self.queued_jobs.push_front(job);
                self.persist_queue_state();
                return Task::none();
            }
            self.persist_queue_state();
            self.start_job(job)
        } else {
            Task::none()
        }
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
        if let Ok(ctx) = self.app_context() {
            let _ = ctx.db.with_db(|database| {
                let repository = JobRepository::new(database);
                repository.record_completed_job_history(&completed_job)
            });
        }
        self.persist_queue_state();
    }
}

fn build_content_refresh_event(
    app_context: Option<AppContext>,
    request_id: u64,
    reason: String,
) -> Message {
    let result = app_context
        .ok_or_else(|| anyhow::anyhow!("Database unavailable"))
        .and_then(|ctx| commands::refresh_content(&ctx))
        .unwrap_or_else(|err| ContentRefreshResult {
            imported_urls: Err(err.to_string()),
            library_articles: Err(err.to_string()),
            library_stats: Err(err.to_string()),
        });
    Message::ContentRefreshCompleted {
        request_id,
        reason,
        result,
    }
}
