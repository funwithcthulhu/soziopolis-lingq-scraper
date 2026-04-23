use super::*;

impl SoziopolisLingqGui {
    pub(super) fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        Panel::left("sidebar")
            .exact_size(240.0)
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(24, 28, 37))
                    .inner_margin(Margin::same(16)),
            )
            .show_inside(ui, |ui| {
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

    pub(super) fn render_top_notice(&mut self, ui: &mut egui::Ui) {
        Panel::top("top_notice")
            .exact_size(if self.notice.is_some() { 36.0 } else { 0.0 })
            .show_inside(ui, |ui| {
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

    pub(super) fn render_lingq_settings_window(&mut self, ctx: &Context) {
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
                    if ui.button("Test / refresh courses").clicked() {
                        self.load_collections();
                    }
                });
            });
        self.show_lingq_settings = open;
    }

    pub(super) fn render_browse_view(&mut self, ui: &mut egui::Ui) {
        let browse_job_active = self.batch_fetching || self.browse_loading;
        let available_sections = SECTIONS.to_vec();
        let imported_count = self
            .browse_articles
            .iter()
            .filter(|article| self.browse_imported_urls.contains(&article.url))
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
                ui.label("Filter");
                ui.add(TextEdit::singleline(&mut self.browse_search).desired_width(180.0));
                ui.label(format!(
                    "Loaded {} / target {}",
                    self.browse_articles.len(),
                    self.browse_limit
                ));
                if ui
                    .add_enabled(!browse_job_active, egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.save_settings();
                    self.refresh_current_browse_scope();
                }
                if ui
                    .add_enabled(!browse_job_active, egui::Button::new("Browse all sections"))
                    .clicked()
                {
                    self.browse_only_new = false;
                    self.save_settings();
                    self.browse_all_sections();
                }
                if ui
                    .add_enabled(
                        !browse_job_active,
                        egui::Button::new("Find new across sections"),
                    )
                    .clicked()
                {
                    self.discover_new_across_sections();
                }
                if ui
                    .add_enabled(
                        !browse_job_active
                            && !(self.browse_scope == BrowseScope::CurrentSection
                                && self.browse_end_reached),
                        egui::Button::new("Load more"),
                    )
                    .clicked()
                {
                    self.browse_limit += 80;
                    match self.browse_scope {
                        BrowseScope::CurrentSection => self.load_more_current_section(),
                        BrowseScope::AllSections => self.load_more_all_sections(),
                    }
                }
                if ui
                    .add_enabled(
                        !browse_job_active,
                        egui::Button::new("Select visible not imported"),
                    )
                    .clicked()
                {
                    self.browse_selected = visible_articles
                        .iter()
                        .filter(|article| !self.browse_imported_urls.contains(&article.url))
                        .map(|article| article.url.clone())
                        .collect();
                }
                if ui
                    .add_enabled_ui(!browse_job_active, |ui| {
                        ui.checkbox(&mut self.browse_only_new, "Only new")
                    })
                    .inner
                    .changed()
                {
                    self.save_settings();
                }
                if ui
                    .add_enabled(!browse_job_active, egui::Button::new("Clear selection"))
                    .clicked()
                {
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
            if self.browse_scope == BrowseScope::CurrentSection && self.browse_end_reached {
                ui.small(
                    RichText::new(format!(
                        "Reached the end of this section at {} unique article(s).",
                        self.browse_articles.len()
                    ))
                    .color(Color32::from_gray(160)),
                );
            }
            if self.browse_only_new {
                let summary =
                    "Only new is showing articles whose URLs are not already in your library."
                        .to_owned();
                ui.small(RichText::new(summary).color(Color32::from_gray(160)));
            }
        });

        ui.add_space(12.0);
        ScrollArea::vertical().show(ui, |ui| {
            for article in visible_articles {
                ui.push_id(article.url.clone(), |ui| {
                    article_card_frame(ui, |ui| {
                        ui.vertical(|ui| {
                            let mut checked = self.browse_selected.contains(&article.url);
                            ui.horizontal_wrapped(|ui| {
                                let selection_response = ui.add_enabled(
                                    !browse_job_active,
                                    egui::Checkbox::without_text(&mut checked),
                                );
                                if selection_response.changed() {
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
                                }
                                if ui
                                    .add_enabled(!browse_job_active, egui::Link::new("Open original"))
                                    .clicked()
                                {
                                    let _ = webbrowser::open(&article.url);
                                }
                                if ui
                                    .add_enabled(!browse_job_active, egui::Link::new("Preview"))
                                    .clicked()
                                {
                                    self.open_preview(article.url.clone());
                                }
                            });
                            ui.small(
                                RichText::new(compact_url(&article.url))
                                    .color(Color32::from_gray(150)),
                            );
                        });
                    });
                });
                ui.add_space(4.0);
            }

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(match self.browse_scope {
                    BrowseScope::CurrentSection => "Need more from this section?",
                    BrowseScope::AllSections => "Need more from all sections?",
                });
                if ui
                    .add_enabled(
                        !browse_job_active
                            && !(self.browse_scope == BrowseScope::CurrentSection
                                && self.browse_end_reached),
                        egui::Button::new("Load 80 more"),
                    )
                    .clicked()
                {
                    self.browse_limit += 80;
                    match self.browse_scope {
                        BrowseScope::CurrentSection => self.load_more_current_section(),
                        BrowseScope::AllSections => self.load_more_all_sections(),
                    }
                }
            });
        });
    }

    pub(super) fn render_library_view(&mut self, ui: &mut egui::Ui) {
        let library_job_active = self.lingq_uploading;
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
                    ui.checkbox(&mut self.library_dense_mode, "Dense mode");
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
            if ui
                .add_enabled(!library_job_active, egui::Button::new("Select all visible"))
                .clicked()
            {
                self.select_all_visible_articles();
            }
            if ui
                .add_enabled(
                    !library_job_active,
                    egui::Button::new("Select all not uploaded"),
                )
                .clicked()
            {
                self.lingq_selected_articles = filtered_articles
                    .iter()
                    .filter(|article| !article.uploaded_to_lingq)
                    .map(|article| article.id)
                    .collect();
            }
            if ui
                .add_enabled(!library_job_active, egui::Button::new("Clear selection"))
                .clicked()
            {
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
                    if self.library_dense_mode {
                        render_library_article_dense_row(self, ui, article);
                    } else {
                        render_library_article_card(self, ui, article);
                    }
                    ui.add_space(4.0);
                }
            } else {
                for article in filtered_articles {
                    if self.library_dense_mode {
                        render_library_article_dense_row(self, ui, article);
                    } else {
                        render_library_article_card(self, ui, article);
                    }
                    ui.add_space(4.0);
                }
            }
        });
    }

    pub(super) fn render_lingq_panel(&mut self, ui: &mut egui::Ui) {
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
        if let Some(progress) = &self.upload_progress {
            ui.add_space(8.0);
            render_upload_progress(ui, progress);
        }
    }

    pub(super) fn render_article_view(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn render_preview_drawer(&mut self, ui: &mut egui::Ui) {
        if !self.show_preview || self.current_view == View::Article {
            return;
        }

        let mut open_full_article = None;
        Panel::right("preview_drawer")
            .default_size(400.0)
            .min_size(320.0)
            .resizable(true)
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(18, 22, 30))
                    .inner_margin(Margin::same(16)),
            )
            .show_inside(ui, |ui| {
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
            self.open_article(stored_article.id);
        }
    }
}
