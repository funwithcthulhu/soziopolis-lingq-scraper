use super::*;

impl SoziopolisLingqGui {
    fn handle_browse_loaded(&mut self, request_id: u64, result: Result<BrowseResponse, String>) {
        if request_id != self.browse_request_id {
            logging::warn(format!(
                "discarded stale browse result for request {request_id}; current request is {}",
                self.browse_request_id
            ));
            return;
        }
        self.browse_loading = false;
        match result {
            Ok(result) => {
                logging::info(format!(
                    "browse result loaded with {} article(s); exhausted={}",
                    result.articles.len(),
                    result.exhausted
                ));
                self.browse_report = Some(result.report);
                self.browse_articles = result.articles;
                self.browse_end_reached = result.exhausted;
                self.browse_session_state = result.session_state;
            }
            Err(err) => self.set_notice(err, NoticeKind::Error),
        }
    }

    fn handle_batch_fetched(
        &mut self,
        job_id: u64,
        saved_count: usize,
        saved_articles: Vec<ArticleListItem>,
        skipped_existing: usize,
        skipped_out_of_range: usize,
        failed: Vec<FailedFetchItem>,
        canceled: bool,
    ) {
        let job_label = self
            .active_job
            .as_ref()
            .map(|job| job.label.clone())
            .unwrap_or_else(|| "Import job".to_owned());
        self.batch_fetching = false;
        self.import_progress = None;
        self.failed_fetches = failed.clone();
        self.apply_imported_articles(saved_articles);
        self.record_completed_job(
            job_id,
            JobKind::Import,
            job_label,
            format_import_result_summary(
                saved_count,
                skipped_existing,
                skipped_out_of_range,
                failed.len(),
                canceled,
                None,
            ),
            failed.is_empty() && !canceled,
        );
        self.active_job = None;
        self.start_next_queued_job();
        self.persist_queue_state();
        if failed.is_empty() {
            self.set_notice(
                format_import_result_summary(
                    saved_count,
                    skipped_existing,
                    skipped_out_of_range,
                    0,
                    canceled,
                    None,
                ),
                if canceled {
                    NoticeKind::Info
                } else {
                    NoticeKind::Success
                },
            );
        } else {
            self.set_notice(
                format_import_result_summary(
                    saved_count,
                    skipped_existing,
                    skipped_out_of_range,
                    failed.len(),
                    canceled,
                    Some(&failed[0].message),
                ),
                if saved_count > 0 {
                    NoticeKind::Info
                } else {
                    NoticeKind::Error
                },
            );
        }
    }

    fn handle_content_refresh_completed(
        &mut self,
        request_id: u64,
        reason: String,
        result: ContentRefreshResult,
    ) {
        if request_id != self.content_refresh_request_id {
            logging::warn(format!(
                "discarded stale content refresh result for request {request_id}; current request is {}",
                self.content_refresh_request_id
            ));
            return;
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
                self.library_search_cache_query.clear();
                self.library_search_cache_results.clear();
            }
            Err(err) => failures.push(format!("library articles: {err}")),
        }

        match result.library_stats {
            Ok(stats) => {
                logging::info(format!(
                    "content refresh {request_id}: stats refreshed; articles={}, uploaded={}, avg_words={}",
                    stats.total_articles, stats.uploaded_articles, stats.average_word_count
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

    fn handle_lingq_logged_in(&mut self, result: Result<String, String>) {
        match result {
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
        }
    }

    fn handle_collections_loaded(&mut self, result: Result<Vec<Collection>, String>) {
        self.lingq_loading_collections = false;
        match result {
            Ok(collections) => {
                let collection_count = collections.len();
                self.lingq_collections = collections;
                self.lingq_connected = true;
                self.save_settings();
                self.start_next_queued_job();
                self.set_notice(
                    format!("Connected to LingQ. Loaded {collection_count} course(s)."),
                    NoticeKind::Success,
                );
            }
            Err(err) => {
                self.lingq_connected = false;
                self.set_notice(err, NoticeKind::Error);
            }
        }
    }

    fn handle_batch_uploaded(
        &mut self,
        job_id: u64,
        uploaded: usize,
        successes: Vec<UploadSuccess>,
        failed: Vec<UploadFailure>,
        canceled: bool,
    ) {
        let job_label = self
            .active_job
            .as_ref()
            .map(|job| job.label.clone())
            .unwrap_or_else(|| "Upload job".to_owned());
        self.lingq_uploading = false;
        self.upload_progress = None;
        self.apply_uploaded_articles(&successes);
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

    pub(super) fn poll_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                AppEvent::BrowseLoaded { request_id, result } => {
                    self.handle_browse_loaded(request_id, result)
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
                    saved_articles,
                    skipped_existing,
                    skipped_out_of_range,
                    failed,
                    canceled,
                } => self.handle_batch_fetched(
                    job_id,
                    saved_count,
                    saved_articles,
                    skipped_existing,
                    skipped_out_of_range,
                    failed,
                    canceled,
                ),
                AppEvent::ContentRefreshCompleted {
                    request_id,
                    reason,
                    result,
                } => self.handle_content_refresh_completed(request_id, reason, result),
                AppEvent::LingqLoggedIn(result) => self.handle_lingq_logged_in(result),
                AppEvent::CollectionsLoaded(result) => self.handle_collections_loaded(result),
                AppEvent::BatchUploaded {
                    job_id,
                    uploaded,
                    successes,
                    failed,
                    canceled,
                } => self.handle_batch_uploaded(job_id, uploaded, successes, failed, canceled),
            }
        }

        if let Some(notice) = &self.notice {
            if notice.created_at.elapsed() > Duration::from_secs(7) {
                self.notice = None;
            }
        }
    }
}
