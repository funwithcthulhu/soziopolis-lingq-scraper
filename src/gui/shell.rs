use super::*;

impl SoziopolisLingqGui {
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let task_runtime = AppTaskRuntime::new(tx.clone());
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
                        SettingsStore::from_parts(
                            fallback_path,
                            crate::settings::AppSettings::default(),
                        )
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
        let (app_context, app_context_error) = match AppContext::shared() {
            Ok(context) => (Some(context), None),
            Err(err) => (None, Some(err.to_string())),
        };

        let mut app = Self {
            app_context,
            app_context_error,
            task_runtime,
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
            browse_view_revision: 0,
            browse_visible_cache_revision: u64::MAX,
            browse_visible_cache_query: String::new(),
            browse_visible_cache_only_new: false,
            browse_visible_cache_indices: Vec::new(),
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
            library_page_size: 120,
            library_data_revision: 0,
            library_search_cache_query: String::new(),
            library_search_cache_results: Vec::new(),
            library_filtered_cache_revision: u64::MAX,
            library_filtered_cache_key: String::new(),
            library_filtered_cache_results: Vec::new(),
            library_page_cache_key: String::new(),
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
        app.refresh_browse();
        app.request_content_refresh("app startup");
        if let Some(message) = settings_notice
            .or(startup_notice)
            .or_else(|| app.app_context_error.clone())
        {
            app.set_notice(message, NoticeKind::Info);
        }
        if app.lingq_connected {
            app.load_collections();
        }
        app.start_next_queued_job();
        app
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar(ui);
    }

    fn top_notice(&mut self, ui: &mut egui::Ui) {
        self.render_top_notice(ui);
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

    pub(super) fn lingq_panel(&mut self, ui: &mut egui::Ui) {
        self.render_lingq_panel(ui);
    }

    fn article_view(&mut self, ui: &mut egui::Ui) {
        self.render_article_view(ui);
    }

    fn diagnostics_view(&mut self, ui: &mut egui::Ui) {
        self.render_diagnostics_view(ui);
    }

    fn preview_drawer(&mut self, ui: &mut egui::Ui) {
        self.render_preview_drawer(ui);
    }
}

impl eframe::App for SoziopolisLingqGui {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.guard_ui_phase("processing background events", |app| app.poll_events());
        ctx.request_repaint_after(Duration::from_millis(150));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.guard_ui_phase("rendering top notice", |app| app.top_notice(ui));
        self.guard_ui_phase("rendering sidebar", |app| app.sidebar(ui));
        self.guard_ui_phase("rendering LingQ settings", |app| {
            app.lingq_settings_window(&ctx)
        });
        self.guard_ui_phase("rendering preview drawer", |app| app.preview_drawer(ui));
        self.guard_ui_phase("rendering main view", |app| {
            egui::CentralPanel::default()
                .frame(
                    Frame::default()
                        .fill(Color32::from_rgb(15, 18, 25))
                        .inner_margin(Margin::same(20)),
                )
                .show_inside(ui, |ui| match app.current_view {
                    View::Browse => app.browse_view(ui),
                    View::Library => app.library_view(ui),
                    View::Article => app.article_view(ui),
                    View::Diagnostics => app.diagnostics_view(ui),
                });
        });
    }
}
