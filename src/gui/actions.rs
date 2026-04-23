use super::*;

impl SoziopolisLingqGui {
    pub(super) fn save_settings(&mut self) {
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

    pub(super) fn request_content_refresh(&mut self, reason: &str) {
        self.library_loading = true;
        self.content_refresh_request_id = self.content_refresh_request_id.wrapping_add(1);
        let request_id = self.content_refresh_request_id;
        let reason = reason.to_owned();
        let app_context = self.app_context().ok();
        let tx = self.tx.clone();
        logging::info(format!(
            "starting content refresh pipeline {request_id} after {reason}"
        ));
        super::tasks::spawn_app_event_task(
            tx,
            "Content refresh worker",
            move |_| build_content_refresh_event_with_context(app_context, request_id, reason),
            move |message| AppEvent::ContentRefreshCompleted {
                request_id,
                reason: "internal refresh error".to_owned(),
                result: ContentRefreshResult {
                    imported_urls: Err(message.clone()),
                    library_articles: Err(message.clone()),
                    library_stats: Err(message),
                },
            },
        );
    }

    pub(super) fn refresh_browse(&mut self) {
        self.browse_scope = BrowseScope::CurrentSection;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        logging::info(format!(
            "browse refresh requested for section '{}' with limit {}",
            self.browse_section, self.browse_limit
        ));
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_end_reached = false;
        self.browse_session_state = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let section = self.browse_section.clone();
        let limit = self.browse_limit;
        super::tasks::spawn_app_event_task(
            tx,
            "Browse section worker",
            move |_| AppEvent::BrowseLoaded {
                request_id,
                result: BrowseService::browse_section(&section, limit).map_err(|err| err.to_string()),
            },
            move |message| AppEvent::BrowseLoaded {
                request_id,
                result: Err(message),
            },
        );
    }

    pub(super) fn browse_all_sections(&mut self) {
        self.browse_scope = BrowseScope::AllSections;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        logging::info(format!(
            "browse all sections requested with total limit {}",
            self.browse_limit
        ));
        self.browse_loading = true;
        self.browse_selected.clear();
        self.browse_report = None;
        self.browse_end_reached = false;
        self.browse_session_state = None;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let limit = self.browse_limit;
        super::tasks::spawn_app_event_task(
            tx,
            "Browse all sections worker",
            move |_| AppEvent::BrowseLoaded {
                request_id,
                result: BrowseService::browse_all_sections(limit).map_err(|err| err.to_string()),
            },
            move |message| AppEvent::BrowseLoaded {
                request_id,
                result: Err(message),
            },
        );
    }

    pub(super) fn discover_new_across_sections(&mut self) {
        self.browse_only_new = true;
        self.save_settings();
        self.browse_all_sections();
    }

    pub(super) fn refresh_current_browse_scope(&mut self) {
        match self.browse_scope {
            BrowseScope::CurrentSection => self.refresh_browse(),
            BrowseScope::AllSections => self.browse_all_sections(),
        }
    }

    pub(super) fn load_more_current_section(&mut self) {
        if self.browse_loading {
            return;
        }

        self.browse_scope = BrowseScope::CurrentSection;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let limit = self.browse_limit;

        match self.browse_session_state.clone() {
            Some(BrowseSessionState::CurrentSection(state)) => {
                super::tasks::spawn_app_event_task(
                    tx,
                    "Browse section continuation worker",
                    move |_| AppEvent::BrowseLoaded {
                        request_id,
                        result: BrowseService::continue_browse_section(state, limit)
                            .map_err(|err| err.to_string()),
                    },
                    move |message| AppEvent::BrowseLoaded {
                        request_id,
                        result: Err(message),
                    },
                );
            }
            _ => self.refresh_browse(),
        }
    }

    pub(super) fn load_more_all_sections(&mut self) {
        if self.browse_loading {
            return;
        }

        self.browse_scope = BrowseScope::AllSections;
        self.browse_scope_label = self.browse_scope.label().to_owned();
        self.browse_loading = true;
        self.browse_request_id = self.browse_request_id.wrapping_add(1);
        let request_id = self.browse_request_id;
        let tx = self.tx.clone();
        let limit = self.browse_limit;

        match self.browse_session_state.clone() {
            Some(BrowseSessionState::AllSections(state)) => {
                super::tasks::spawn_app_event_task(
                    tx,
                    "Browse all sections continuation worker",
                    move |_| AppEvent::BrowseLoaded {
                        request_id,
                        result: BrowseService::continue_browse_all_sections(state, limit)
                            .map_err(|err| err.to_string()),
                    },
                    move |message| AppEvent::BrowseLoaded {
                        request_id,
                        result: Err(message),
                    },
                );
            }
            _ => self.browse_all_sections(),
        }
    }

    pub(super) fn open_preview(&mut self, url: String) {
        logging::info(format!("opening remote preview for {}", url));
        self.preview_loading = true;
        self.show_preview = true;
        self.preview_article = None;
        self.preview_stored_article = None;
        let tx = self.tx.clone();
        super::tasks::spawn_app_event_task(
            tx,
            "Preview worker",
            move |_| AppEvent::PreviewLoaded(
                BrowseService::preview_article(&url).map_err(|err| err.to_string()),
            ),
            move |message| AppEvent::PreviewLoaded(Err(message)),
        );
    }

    pub(super) fn open_library_preview(&mut self, article_id: i64) {
        logging::info(format!("opening stored preview for article #{article_id}"));
        match self
            .app_context()
            .map_err(anyhow::Error::msg)
            .and_then(|ctx| commands::get_article_detail(&ctx, article_id))
        {
            Ok(Some(article)) => {
                self.preview_loading = false;
                self.show_preview = true;
                self.preview_article = Some(stored_article_to_preview_article(&article));
                self.preview_stored_article = Some(article);
            }
            Ok(None) => {
                self.set_notice("Article not found in the local library.", NoticeKind::Error)
            }
            Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
        }
    }

    pub(super) fn select_all_visible_articles(&mut self) {
        match self.filtered_library_articles() {
            Ok(articles) => {
                self.lingq_selected_articles =
                    articles.into_iter().map(|article| article.id).collect();
            }
            Err(err) => self.set_notice(err, NoticeKind::Error),
        }
    }

    pub(super) fn load_collections(&mut self) {
        if self.lingq_api_key.trim().is_empty() {
            self.set_notice("Enter a LingQ API key first.", NoticeKind::Error);
            return;
        }
        logging::info("loading LingQ collections");
        self.lingq_loading_collections = true;
        let tx = self.tx.clone();
        let api_key = self.lingq_api_key.clone();
        super::tasks::spawn_app_event_task(
            tx,
            "LingQ collections worker",
            move |_| AppEvent::CollectionsLoaded(
                LingqService::collections(&api_key, "de").map_err(|err| err.to_string()),
            ),
            move |message| AppEvent::CollectionsLoaded(Err(message)),
        );
    }

    pub(super) fn login_to_lingq(&mut self) {
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
        self.lingq_password.clear();
        super::tasks::spawn_app_event_task(
            tx,
            "LingQ login worker",
            move |_| AppEvent::LingqLoggedIn(
                LingqService::login(&username, &password).map_err(|err| err.to_string()),
            ),
            move |message| AppEvent::LingqLoggedIn(Err(message)),
        );
    }

    pub(super) fn batch_upload_selected(&mut self) {
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

    pub(super) fn open_article(&mut self, article_id: i64) {
        match self
            .app_context()
            .map_err(anyhow::Error::msg)
            .and_then(|ctx| commands::get_article_detail(&ctx, article_id))
        {
            Ok(Some(article)) => {
                self.article_detail = Some(article);
                self.current_view = View::Article;
                self.save_settings();
            }
            Ok(None) => {
                self.set_notice("Article not found in the local library.", NoticeKind::Error)
            }
            Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
        }
    }
}
