use super::*;

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Navigation ──────────────────────────────────────
            Message::SwitchView(view) => {
                self.current_view = view;
                self.save_settings();
                Task::none()
            }
            Message::ToggleLingqSettings => {
                self.show_lingq_settings = !self.show_lingq_settings;
                Task::none()
            }
            Message::ClosePreview => {
                self.show_preview = false;
                Task::none()
            }
            Message::Tick => {
                // Expire notices after 7 seconds
                if let Some(notice) = &self.notice {
                    if notice.created_at.elapsed() > Duration::from_secs(7) {
                        self.notice = None;
                    }
                }
                Task::none()
            }
            Message::NoticeExpired => {
                self.notice = None;
                Task::none()
            }
            Message::Noop => Task::none(),

            // ── Browse ──────────────────────────────────────────
            Message::BrowseSectionChanged(section) => {
                self.browse_section = section;
                self.browse_limit = 80;
                self.save_settings();
                self.spawn_browse_refresh()
            }
            Message::BrowseSearchChanged(search) => {
                self.browse_search = search;
                Task::none()
            }
            Message::BrowseToggleOnlyNew(only_new) => {
                self.browse_only_new = only_new;
                self.save_settings();
                Task::none()
            }
            Message::BrowseToggleArticle(url) => {
                if self.browse_selected.contains(&url) {
                    self.browse_selected.remove(&url);
                } else {
                    self.browse_selected.insert(url);
                }
                Task::none()
            }
            Message::BrowseRefresh => {
                self.save_settings();
                match self.browse_scope {
                    BrowseScope::CurrentSection => self.spawn_browse_refresh(),
                    BrowseScope::AllSections => self.spawn_browse_all_sections(),
                }
            }
            Message::BrowseAllSections => {
                self.browse_only_new = false;
                self.save_settings();
                self.spawn_browse_all_sections()
            }
            Message::BrowseFindNew => {
                self.browse_only_new = true;
                self.save_settings();
                self.spawn_browse_all_sections()
            }
            Message::BrowseLoadMore => {
                self.browse_limit += 80;
                match self.browse_scope {
                    BrowseScope::CurrentSection => self.spawn_load_more_current_section(),
                    BrowseScope::AllSections => self.spawn_load_more_all_sections(),
                }
            }
            Message::BrowseSelectVisibleNew => {
                let search = self.browse_search.trim().to_lowercase();
                self.browse_selected = self
                    .browse_articles
                    .iter()
                    .filter(|a| {
                        !self.browse_imported_urls.contains(&a.url)
                            && (search.is_empty() || article_matches_search(a, &search))
                    })
                    .map(|a| a.url.clone())
                    .collect();
                Task::none()
            }
            Message::BrowseClearSelection => {
                self.browse_selected.clear();
                Task::none()
            }
            Message::BrowseFetchSelected => {
                let selected_urls: HashSet<String> = self.browse_selected.clone();
                let mut articles: Vec<ArticleSummary> = self
                    .browse_articles
                    .iter()
                    .filter(|a| selected_urls.contains(&a.url))
                    .cloned()
                    .collect();
                for url in &selected_urls {
                    if !articles.iter().any(|a| a.url == *url) {
                        articles.push(ArticleSummary {
                            url: url.clone(),
                            title: String::new(),
                            teaser: String::new(),
                            author: String::new(),
                            date: String::new(),
                            section: String::new(),
                            source_kind: crate::soziopolis::DiscoverySourceKind::Section,
                            source_label: String::new(),
                        });
                    }
                }
                if articles.is_empty() {
                    self.set_notice("Select at least one article.", NoticeKind::Error);
                    return Task::none();
                }
                let total = articles.len();
                let job = QueuedJob {
                    id: self.next_job_id(),
                    kind: JobKind::Import,
                    label: format!("Import {} article(s)", total),
                    total,
                    request: QueuedJobRequest::Import { articles },
                };
                self.enqueue_job(job)
            }
            Message::BrowseLoaded { request_id, result } => {
                if request_id != self.browse_request_id {
                    return Task::none();
                }
                self.browse_loading = false;
                match result {
                    Ok(resp) => {
                        self.browse_report = Some(resp.report);
                        self.browse_articles = resp.articles;
                        self.browse_end_reached = resp.exhausted;
                        self.browse_session_state = resp.session_state;
                    }
                    Err(err) => self.set_task_error_notice(err),
                }
                Task::none()
            }

            // ── Preview ─────────────────────────────────────────
            Message::OpenPreview(url) => self.spawn_open_preview(url),
            Message::OpenLibraryPreview(id) => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::get_article_detail(&ctx, id))
                {
                    Ok(Some(article)) => {
                        self.preview_loading = false;
                        self.show_preview = true;
                        self.preview_article = Some(stored_article_to_preview_article(&article));
                        self.preview_stored_article = Some(article);
                    }
                    Ok(None) => self.set_notice("Article not found.", NoticeKind::Error),
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::PreviewLoaded(result) => {
                self.preview_loading = false;
                match result {
                    Ok((article, stored)) => {
                        self.preview_article = Some(article);
                        self.preview_stored_article = stored;
                    }
                    Err(err) => {
                        self.show_preview = false;
                        self.set_task_error_notice(err);
                    }
                }
                Task::none()
            }
            Message::OpenFullArticle(id) => {
                self.show_preview = false;
                self.update(Message::OpenArticle(id))
            }
            Message::OpenArticle(id) => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::get_article_detail(&ctx, id))
                {
                    Ok(Some(article)) => {
                        self.article_detail = Some(article);
                        self.current_view = View::Article;
                        self.save_settings();
                    }
                    Ok(None) => self.set_notice("Article not found.", NoticeKind::Error),
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }

            // ── Library ─────────────────────────────────────────
            Message::LibrarySearchChanged(s) => {
                self.library_search = s;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryTopicChanged(t) => {
                self.library_topic = t;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryToggleNotUploaded(v) => {
                self.library_only_not_uploaded = v;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryMinWordsChanged(s) => {
                self.library_word_count_min = s;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryMaxWordsChanged(s) => {
                self.library_word_count_max = s;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibrarySortChanged(mode) => {
                self.library_sort_mode = mode;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryToggleDense(v) => {
                self.library_dense_mode = v;
                Task::none()
            }
            Message::LibraryToggleGroupByTopic(v) => {
                self.library_group_by_topic = v;
                self.invalidate_library_view_cache();
                Task::none()
            }
            Message::LibraryToggleFilters => {
                self.library_filters_expanded = !self.library_filters_expanded;
                Task::none()
            }
            Message::LibraryRefresh => self.spawn_content_refresh("manual library refresh"),
            Message::LibrarySelectAllVisible => {
                self.select_all_visible_articles();
                Task::none()
            }
            Message::LibrarySelectAllNotUploaded => {
                if let Err(err) = self.select_all_matching_not_uploaded() {
                    self.set_notice(err, NoticeKind::Error);
                }
                Task::none()
            }
            Message::LibraryClearSelection => {
                self.lingq_selected_articles.clear();
                Task::none()
            }
            Message::LibraryToggleArticle(id) => {
                if self.lingq_selected_articles.contains(&id) {
                    self.lingq_selected_articles.remove(&id);
                } else {
                    self.lingq_selected_articles.insert(id);
                }
                Task::none()
            }
            Message::LibraryDeleteArticle(id) => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::delete_article(&ctx, id))
                {
                    Ok(_) => {
                        self.remove_article_from_local_state(id);
                        self.set_notice("Article deleted.", NoticeKind::Info);
                    }
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::LibraryNextPage => {
                self.library_page_index += 1;
                self.library_page_cache_key.clear();
                self.library_page_cache = None;
                Task::none()
            }
            Message::LibraryPrevPage => {
                self.library_page_index = self.library_page_index.saturating_sub(1);
                self.library_page_cache_key.clear();
                self.library_page_cache = None;
                Task::none()
            }

            // ── Article detail ──────────────────────────────────
            Message::ArticleBack => {
                self.current_view = View::Library;
                self.save_settings();
                Task::none()
            }
            Message::ArticleCopyText => {
                if let Some(article) = &self.article_detail {
                    let text = article.clean_text.clone();
                    self.set_notice("Article copied to clipboard.", NoticeKind::Success);
                    clipboard::write(text)
                } else {
                    Task::none()
                }
            }

            // ── LingQ auth ─────────────────────────────────────
            Message::LingqAuthModeChanged(mode) => {
                self.lingq_auth_mode = mode;
                Task::none()
            }
            Message::LingqUsernameChanged(s) => {
                self.lingq_username = s;
                Task::none()
            }
            Message::LingqPasswordChanged(s) => {
                self.lingq_password = s;
                Task::none()
            }
            Message::LingqApiKeyChanged(s) => {
                self.lingq_api_key = s;
                Task::none()
            }
            Message::LingqConnect => {
                if self.persist_lingq_api_key() {
                    self.spawn_load_collections()
                } else {
                    Task::none()
                }
            }
            Message::LingqDisconnect => {
                if self.clear_stored_lingq_api_key() {
                    self.lingq_connected = false;
                    self.lingq_collections.clear();
                }
                Task::none()
            }
            Message::LingqSignIn => self.spawn_login_to_lingq(),
            Message::LingqCollectionChanged(id) => {
                self.lingq_selected_collection = id;
                self.save_settings();
                Task::none()
            }
            Message::LingqRefreshCollections => self.spawn_load_collections(),
            Message::LingqLoggedIn(result) => match result {
                Ok(token) => {
                    self.lingq_api_key = token;
                    self.lingq_password.clear();
                    if self.persist_lingq_api_key() {
                        self.spawn_load_collections()
                    } else {
                        self.lingq_loading_collections = false;
                        Task::none()
                    }
                }
                Err(err) => {
                    self.lingq_loading_collections = false;
                    self.set_task_error_notice(err);
                    Task::none()
                }
            },
            Message::CollectionsLoaded(result) => {
                self.lingq_loading_collections = false;
                match result {
                    Ok(collections) => {
                        let count = collections.len();
                        self.lingq_collections = collections;
                        self.lingq_connected = true;
                        self.save_settings();
                        let queue_task = self.try_start_next_queued_job();
                        self.set_notice(
                            format!("Connected to LingQ. {count} course(s) loaded."),
                            NoticeKind::Success,
                        );
                        queue_task
                    }
                    Err(err) => {
                        self.lingq_connected = false;
                        self.set_task_error_notice(err);
                        Task::none()
                    }
                }
            }

            // ── LingQ upload selection ──────────────────────────
            Message::LingqClearUploadSelection => {
                self.lingq_selected_articles.clear();
                Task::none()
            }
            Message::LingqUploadSelected => {
                if self.lingq_api_key.trim().is_empty() {
                    self.set_notice("Connect to LingQ first.", NoticeKind::Error);
                    return Task::none();
                }
                let ids: Vec<i64> = self.lingq_selected_articles.iter().copied().collect();
                let collection_id = self.lingq_selected_collection;
                if ids.is_empty() {
                    self.set_notice("Select articles to upload.", NoticeKind::Error);
                    return Task::none();
                }
                let total = ids.len();
                let job = QueuedJob {
                    id: self.next_job_id(),
                    kind: JobKind::Upload,
                    label: format!("Upload {} article(s) to LingQ", total),
                    total,
                    request: QueuedJobRequest::Upload { ids, collection_id },
                };
                self.save_settings();
                self.enqueue_job(job)
            }

            // ── Background task results ─────────────────────────
            Message::ImportProgress(progress) => {
                if let Some(job) = &mut self.active_job {
                    if job.kind == JobKind::Import {
                        job.processed = progress.processed;
                        job.succeeded = progress.saved_count;
                        job.failed = progress.failed_count;
                        job.current_item = progress.current_item.clone();
                    }
                }
                self.import_progress = Some(progress);
                Task::none()
            }
            Message::BatchFetched {
                job_id,
                saved_count,
                saved_articles,
                skipped_existing,
                skipped_out_of_range,
                failed,
                canceled,
            } => {
                let job_label = self
                    .active_job
                    .as_ref()
                    .map(|j| j.label.clone())
                    .unwrap_or_else(|| "Import job".to_owned());
                if let Some(internal_failure) = failed
                    .first()
                    .filter(|item| item.category == "internal")
                {
                    self.record_task_failure(AppError::internal_task(
                        "import",
                        &job_label,
                        internal_failure.message.clone(),
                    ));
                }
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
                let queue_task = self.try_start_next_queued_job();
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
                queue_task
            }
            Message::UploadProgressMsg { job_id, progress } => {
                if let Some(job) = &mut self.active_job {
                    if job.id == job_id && job.kind == JobKind::Upload {
                        job.processed = progress.processed;
                        job.total = progress.total;
                        job.succeeded = progress.uploaded;
                        job.failed = progress.failed_count;
                        job.current_item = progress.current_item.clone();
                    }
                }
                self.upload_progress = Some(progress);
                Task::none()
            }
            Message::BatchUploaded {
                job_id,
                uploaded,
                successes,
                failed,
                canceled,
            } => {
                let job_label = self
                    .active_job
                    .as_ref()
                    .map(|j| j.label.clone())
                    .unwrap_or_else(|| "Upload job".to_owned());
                if let Some(internal_failure) = failed
                    .first()
                    .filter(|item| item.article_id == 0 && item.title == "Internal task")
                {
                    self.record_task_failure(AppError::internal_task(
                        "upload",
                        &job_label,
                        internal_failure.message.clone(),
                    ));
                }
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
                let queue_task = self.try_start_next_queued_job();
                self.persist_queue_state();

                if failed.is_empty() {
                    self.set_notice(
                        if canceled {
                            format!("Upload canceled after {uploaded} article(s).")
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
                            "Uploaded {uploaded}, {} failed. {}",
                            failed.len(),
                            failed[0].message
                        ),
                        if uploaded > 0 {
                            NoticeKind::Info
                        } else {
                            NoticeKind::Error
                        },
                    );
                }
                queue_task
            }
            Message::ContentRefreshCompleted {
                request_id,
                reason,
                result,
            } => {
                if request_id != self.content_refresh_request_id {
                    return Task::none();
                }
                self.library_loading = false;
                let mut failures = Vec::new();
                match result.imported_urls {
                    Ok(urls) => {
                        self.browse_imported_urls = urls;
                    }
                    Err(err) => failures.push(err),
                }
                match result.library_articles {
                    Ok(articles) => {
                        self.library_articles = articles;
                        self.invalidate_library_search_cache();
                        self.invalidate_library_view_cache();
                    }
                    Err(err) => failures.push(err),
                }
                match result.library_stats {
                    Ok(stats) => self.library_stats = Some(stats),
                    Err(err) => failures.push(err),
                }
                if !failures.is_empty() {
                    self.set_notice(
                        format!("Refresh after {reason}: {}", failures[0]),
                        if failures.len() == 3 {
                            NoticeKind::Error
                        } else {
                            NoticeKind::Info
                        },
                    );
                }
                Task::none()
            }
            Message::ContentRefreshFailed {
                request_id,
                reason,
                error,
            } => {
                if request_id != self.content_refresh_request_id {
                    return Task::none();
                }
                self.library_loading = false;
                self.set_task_error_notice(
                    error.with_details(format!("Refresh trigger: {reason}")),
                );
                Task::none()
            }

            // ── Job queue ───────────────────────────────────────
            Message::CancelActiveJob => {
                if let Some(job) = &self.active_job {
                    job.cancel_flag.store(true, Ordering::Relaxed);
                    self.set_notice(
                        format!("Cancel requested for {}.", job.label),
                        NoticeKind::Info,
                    );
                }
                Task::none()
            }
            Message::PauseQueue => {
                self.queue_paused = true;
                self.persist_queue_state();
                self.set_notice("Queue paused.", NoticeKind::Info);
                Task::none()
            }
            Message::ResumeQueue => {
                self.queue_paused = false;
                self.persist_queue_state();
                self.set_notice("Queue resumed.", NoticeKind::Success);
                self.try_start_next_queued_job()
            }
            Message::RunQueuedUploadNow => {
                if self.active_job.is_some() {
                    self.set_notice("A job is already running.", NoticeKind::Info);
                    return Task::none();
                }
                if let Some(idx) = self
                    .queued_jobs
                    .iter()
                    .position(|j| matches!(j.request, QueuedJobRequest::Upload { .. }))
                {
                    if let Some(job) = self.queued_jobs.remove(idx) {
                        self.persist_queue_state();
                        return self.start_job(job);
                    }
                }
                self.set_notice("No queued upload to run.", NoticeKind::Info);
                Task::none()
            }
            Message::ClearQueuedJobs => {
                self.queued_jobs.clear();
                self.persist_queue_state();
                self.set_notice("Cleared queued jobs.", NoticeKind::Info);
                Task::none()
            }
            Message::RetryFailedImports => {
                if self.failed_fetches.is_empty() {
                    self.set_notice("No failed items to retry.", NoticeKind::Info);
                    return Task::none();
                }
                let articles: Vec<ArticleSummary> = self
                    .failed_fetches
                    .iter()
                    .filter(|item| !item.url.trim().is_empty())
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
                    .collect();
                if articles.is_empty() {
                    self.set_notice(
                        "No retryable import URLs are available. Check Diagnostics for the internal task failure.",
                        NoticeKind::Error,
                    );
                    return Task::none();
                }
                let total = articles.len();
                let job = QueuedJob {
                    id: self.next_job_id(),
                    kind: JobKind::Import,
                    label: format!("Retry {} failed import(s)", total),
                    total,
                    request: QueuedJobRequest::Import { articles },
                };
                self.enqueue_job(job)
            }
            Message::RetryFailedUploads => {
                if self.last_failed_uploads.is_empty() {
                    self.set_notice("No failed uploads to retry.", NoticeKind::Info);
                    return Task::none();
                }
                let ids: Vec<i64> = self
                    .last_failed_uploads
                    .iter()
                    .filter_map(|item| (item.article_id > 0).then_some(item.article_id))
                    .collect();
                if ids.is_empty() {
                    self.set_notice(
                        "No retryable uploads are available. Check Diagnostics for the internal task failure.",
                        NoticeKind::Error,
                    );
                    return Task::none();
                }
                let collection = self.lingq_selected_collection;
                let total = ids.len();
                let job = QueuedJob {
                    id: self.next_job_id(),
                    kind: JobKind::Upload,
                    label: format!("Retry {} failed upload(s)", total),
                    total,
                    request: QueuedJobRequest::Upload {
                        ids,
                        collection_id: collection,
                    },
                };
                self.enqueue_job(job)
            }

            // ── Diagnostics ─────────────────────────────────────
            Message::SelectDiagnosticsJob(id) => {
                self.diagnostics_selected_job_id = Some(id);
                Task::none()
            }
            Message::OpenDataFolder => {
                if let Ok(path) = app_paths::data_dir() {
                    if let Err(err) = open_path_in_explorer(&path) {
                        self.set_notice(err, NoticeKind::Error);
                    }
                }
                Task::none()
            }
            Message::OpenLogFile => {
                if let Ok(path) = app_paths::app_log_path() {
                    if let Err(err) = open_log_in_notepad(&path) {
                        self.set_notice(err, NoticeKind::Error);
                    }
                }
                Task::none()
            }
            Message::CopyRecentLog => match read_recent_log_excerpt(30) {
                Ok(log_text) => {
                    self.set_notice("Copied recent log lines.", NoticeKind::Success);
                    clipboard::write(log_text)
                }
                Err(err) => {
                    self.set_notice(err, NoticeKind::Error);
                    Task::none()
                }
            },
            Message::CreateSupportBundle => {
                match create_support_bundle(self) {
                    Ok(path) => {
                        let _ = open_path_in_explorer(&path);
                        self.set_notice(
                            format!("Created support bundle at {}.", path.display()),
                            NoticeKind::Success,
                        );
                    }
                    Err(err) => self.set_notice(err, NoticeKind::Error),
                }
                Task::none()
            }
            Message::ClearBrowseCache => {
                match commands::clear_browse_cache() {
                    Ok(removed) => self.set_notice(
                        format!("Cleared {removed} cached file(s)."),
                        NoticeKind::Success,
                    ),
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::CompactLocalData => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::compact_local_data(&ctx))
                {
                    Ok(()) => self.set_notice("Compacted local database.", NoticeKind::Success),
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::RebuildSearchIndex => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::rebuild_search_index(&ctx))
                {
                    Ok(()) => self.set_notice("Rebuilt search index.", NoticeKind::Success),
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::VerifyDatabase => {
                match self
                    .app_context()
                    .map_err(anyhow::Error::msg)
                    .and_then(|ctx| commands::verify_database(&ctx))
                {
                    Ok(result) => {
                        self.set_notice(format!("Integrity check: {result}"), NoticeKind::Info)
                    }
                    Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                }
                Task::none()
            }
            Message::ClearTaskFailures => {
                self.recent_task_failures.clear();
                self.set_notice("Cleared task failures.", NoticeKind::Info);
                Task::none()
            }
            Message::OpenUrl(url) => {
                let _ = webbrowser::open(&url);
                Task::none()
            }
        }
    }

    // ── Internal helpers used by update ──────────────────────────

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
                self.set_notice(format!("Could not save token: {err}"), NoticeKind::Error);
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
                self.set_notice(format!("Could not remove token: {err}"), NoticeKind::Error);
                false
            }
        }
    }

    pub(super) fn invalidate_library_search_cache(&mut self) {
        self.library_search_cache_query.clear();
        self.library_search_cache_results.clear();
    }

    pub(super) fn invalidate_library_view_cache(&mut self) {
        self.library_data_revision = self.library_data_revision.wrapping_add(1);
        self.library_filtered_cache_revision = u64::MAX;
        self.library_filtered_cache_key.clear();
        self.library_filtered_cache_results.clear();
        self.library_page_cache_key.clear();
        self.library_page_cache = None;
    }

    fn apply_imported_articles(&mut self, mut saved_articles: Vec<ArticleListItem>) {
        for article in &saved_articles {
            self.browse_imported_urls.insert(article.url.clone());
        }
        self.library_articles
            .retain(|existing| !saved_articles.iter().any(|s| s.id == existing.id));
        self.library_articles.append(&mut saved_articles);
        self.invalidate_library_search_cache();
        self.invalidate_library_view_cache();
        self.library_stats = Some(compute_local_library_stats(&self.library_articles));
    }

    fn apply_uploaded_articles(&mut self, successes: &[UploadSuccess]) {
        for success in successes {
            if let Some(article) = self
                .library_articles
                .iter_mut()
                .find(|a| a.id == success.article_id)
            {
                article.uploaded_to_lingq = true;
                article.lingq_lesson_id = Some(success.lesson_id);
                article.lingq_lesson_url = success.lesson_url.clone();
            }
        }
        self.invalidate_library_search_cache();
        self.invalidate_library_view_cache();
        self.library_stats = Some(compute_local_library_stats(&self.library_articles));
    }

    fn remove_article_from_local_state(&mut self, article_id: i64) {
        let removed_urls: Vec<String> = self
            .library_articles
            .iter()
            .filter(|a| a.id == article_id)
            .map(|a| a.url.clone())
            .collect();
        self.library_articles.retain(|a| a.id != article_id);
        for url in removed_urls {
            self.browse_imported_urls.remove(&url);
        }
        self.lingq_selected_articles.remove(&article_id);
        self.article_detail = self.article_detail.take().filter(|a| a.id != article_id);
        self.invalidate_library_search_cache();
        self.invalidate_library_view_cache();
        self.library_stats = Some(compute_local_library_stats(&self.library_articles));
    }

    fn select_all_visible_articles(&mut self) {
        if let Ok(filtered) = self.get_filtered_library_articles() {
            self.lingq_selected_articles = filtered.iter().map(|a| a.id).collect();
        }
    }

    fn select_all_matching_not_uploaded(&mut self) -> Result<(), String> {
        let filtered = self.get_filtered_library_articles()?;
        self.lingq_selected_articles = filtered
            .iter()
            .filter(|a| !a.uploaded_to_lingq)
            .map(|a| a.id)
            .collect();
        Ok(())
    }

    pub(super) fn get_filtered_library_articles(&mut self) -> Result<Vec<ArticleListItem>, String> {
        let min_words = parse_optional_positive_usize(&self.library_word_count_min, "Min words")?;
        let max_words = parse_optional_positive_usize(&self.library_word_count_max, "Max words")?;

        let trimmed_search = self.library_search.trim();
        let mut articles = if trimmed_search.is_empty() {
            self.library_articles.clone()
        } else {
            if self.library_search_cache_query != trimmed_search {
                if let Ok(ctx) = self.app_context() {
                    self.library_search_cache_results = ctx
                        .db
                        .with_db(|db| {
                            let repository = ArticleRepository::new(db);
                            repository.list_article_cards(Some(trimmed_search), None, false)
                        })
                        .map_err(|err| err.to_string())?;
                    self.library_search_cache_query = trimmed_search.to_owned();
                }
            }
            self.library_search_cache_results.clone()
        };

        articles.retain(|article| {
            (self.library_topic.trim().is_empty()
                || effective_topic_for_article(article) == self.library_topic)
                && (!self.library_only_not_uploaded || !article.uploaded_to_lingq)
                && min_words.is_none_or(|min| article.word_count as usize >= min)
                && max_words.is_none_or(|max| article.word_count as usize <= max)
        });

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

    #[allow(dead_code)]
    fn select_lingq_articles_by_word_count(&mut self) {
        let min = parse_optional_positive_usize(&self.lingq_word_count_min, "Min words");
        let max = parse_optional_positive_usize(&self.lingq_word_count_max, "Max words");
        let (min, max) = match (min, max) {
            (Ok(min), Ok(max)) => (min, max),
            (Err(err), _) | (_, Err(err)) => {
                self.set_notice(err, NoticeKind::Error);
                return;
            }
        };

        self.lingq_selected_articles = self
            .library_articles
            .iter()
            .filter(|a| {
                (!self.lingq_select_only_not_uploaded || !a.uploaded_to_lingq)
                    && min.is_none_or(|m| a.word_count as usize >= m)
                    && max.is_none_or(|m| a.word_count as usize <= m)
            })
            .map(|a| a.id)
            .collect();

        self.set_notice(
            format!(
                "Selected {} article(s) for upload.",
                self.lingq_selected_articles.len()
            ),
            NoticeKind::Info,
        );
    }
}

pub(super) fn article_matches_search(article: &ArticleSummary, search: &str) -> bool {
    [
        article.title.as_str(),
        article.teaser.as_str(),
        article.author.as_str(),
        article.date.as_str(),
        article.section.as_str(),
        article.url.as_str(),
    ]
    .iter()
    .any(|field| field.to_lowercase().contains(search))
}

fn create_support_bundle(app: &App) -> Result<PathBuf, String> {
    let bundles_dir = app_paths::support_bundles_dir().map_err(|e| e.to_string())?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "now".to_owned());
    let bundle_dir = bundles_dir.join(format!("support-bundle-{timestamp}"));
    fs::create_dir_all(&bundle_dir).map_err(|e| e.to_string())?;

    let mut summary = vec![format!("Soziopolis Reader {}", env!("CARGO_PKG_VERSION"))];
    summary.push(format!("Current view: {}", app.current_view.as_str()));
    summary.push(format!("Library articles: {}", app.library_articles.len()));
    summary.push(format!("Browse articles: {}", app.browse_articles.len()));

    fs::write(bundle_dir.join("README.txt"), summary.join("\r\n")).map_err(|e| e.to_string())?;

    if let Ok(path) = app_paths::settings_path() {
        if path.exists() {
            let _ = fs::copy(&path, bundle_dir.join("settings.json"));
        }
    }
    if let Ok(path) = app_paths::app_log_path() {
        if path.exists() {
            let _ = fs::copy(&path, bundle_dir.join("soziopolis-reader.log"));
        }
    }
    if let Ok(path) = app_paths::database_path() {
        if path.exists() {
            let _ = fs::copy(&path, bundle_dir.join("soziopolis_lingq_tool.db"));
        }
    }

    Ok(bundle_dir)
}
