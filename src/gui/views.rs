use super::*;

// ── Main view dispatch ─────────────────────────────────────────────

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let content: Element<'_, Message> = if self.show_lingq_settings {
            self.view_lingq_settings()
        } else {
            match self.current_view {
                View::Browse => self.view_browse(),
                View::Library => self.view_library(),
                View::Article => self.view_article(),
                View::Diagnostics => self.view_diagnostics(),
            }
        };

        let main_area: Element<Message> =
            if self.show_preview && !self.show_lingq_settings && self.current_view != View::Article
            {
                let preview = self.view_preview_drawer();
                row![content, preview].into()
            } else {
                content
            };

        let mut main_col = Column::new();
        if let Some(notice) = &self.notice {
            main_col = main_col.push(self.view_notice(notice));
        }
        main_col = main_col.push(main_area);

        row![sidebar, main_col.width(Length::Fill)]
            .height(Length::Fill)
            .into()
    }

    // ── Notice bar ─────────────────────────────────────────────────

    fn view_notice<'a>(&'a self, notice: &'a Notice) -> Element<'a, Message> {
        let color = notice_color(notice.kind);
        container(text(&notice.message).color(color).size(14))
            .padding(8)
            .width(Length::Fill)
            .into()
    }

    // ── Sidebar ────────────────────────────────────────────────────

    fn view_sidebar(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4).padding(16).width(240);

        col = col.push(text("Soziopolis Reader").size(24));
        col = col.push(Space::with_height(16));

        for (view, label) in [
            (View::Browse, "Browse Articles"),
            (View::Library, "My Library"),
            (View::Diagnostics, "Diagnostics"),
        ] {
            let btn = if self.current_view == view {
                button(text(label).size(14))
                    .style(button::primary)
                    .width(Length::Fill)
            } else {
                button(text(label).size(14))
                    .style(button::secondary)
                    .width(Length::Fill)
            };
            col = col.push(btn.on_press(Message::SwitchView(view)));
        }

        col = col.push(Space::with_height(8));
        col = col.push(
            button(text("LingQ Settings").size(14))
                .style(button::secondary)
                .width(Length::Fill)
                .on_press(Message::ToggleLingqSettings),
        );

        // Library stats in sidebar
        if self.current_view == View::Library {
            col = col.push(Space::with_height(8));
            col = col.push(horizontal_rule(1));
            col = col.push(Space::with_height(6));
            col = col.push(text("Library Stats").size(14));
            col = col.push(Space::with_height(4));
            if let Some(stats) = &self.library_stats {
                col = col.push(sidebar_stat("Articles", stats.total_articles));
                let pct = if stats.total_articles > 0 {
                    (stats.uploaded_articles as f64 / stats.total_articles as f64 * 100.0).round()
                        as i64
                } else {
                    0
                };
                col = col.push(
                    text(format!("Uploaded:  {} ({}%)", stats.uploaded_articles, pct))
                        .size(12)
                        .color(TEXT_SECONDARY),
                );
                col = col.push(sidebar_stat("Avg words", stats.average_word_count));
                let total_words = stats.total_articles * stats.average_word_count;
                col = col.push(
                    text(format!("Total words:  {}", total_words))
                        .size(12)
                        .color(TEXT_SECONDARY),
                );
            } else {
                col = col.push(text("Loading...").size(12).color(TEXT_DIM));
            }
        }

        // Failed imports in sidebar (browse view)
        if self.current_view == View::Browse {
            col = col.push(Space::with_height(8));
            col = col.push(horizontal_rule(1));
            col = col.push(Space::with_height(6));
            col = col.push(text("Failed Imports").size(14));
            col = col.push(
                text(format!("{} retained.", self.failed_fetches.len()))
                    .size(12)
                    .color(TEXT_DIM),
            );
            let mut btn_row = Row::new().spacing(6);
            if !self.failed_fetches.is_empty() && !self.batch_fetching {
                btn_row = btn_row.push(
                    button(text("Retry").size(12))
                        .style(button::secondary)
                        .on_press(Message::RetryFailedImports),
                );
            }
            col = col.push(btn_row);
            let mut fail_col = Column::new().spacing(2);
            if self.failed_fetches.is_empty() {
                fail_col = fail_col.push(text("No failed imports.").size(11).color(TEXT_DIM));
            } else {
                for item in self.failed_fetches.iter().take(15) {
                    let label = if item.title.is_empty() {
                        &item.url
                    } else {
                        &item.title
                    };
                    fail_col = fail_col.push(
                        text(format!(
                            "[{}] {}",
                            item.category,
                            truncate_for_ui(label, 30)
                        ))
                        .size(11)
                        .color(TEXT_DIM),
                    );
                }
            }
            col = col.push(fail_col);
        }

        col = col.push(Space::with_height(Length::Fill));

        container(col)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SIDEBAR)),
                ..Default::default()
            })
            .into()
    }

    // ── Browse view ────────────────────────────────────────────────

    fn view_browse(&self) -> Element<'_, Message> {
        let browse_busy = self.batch_fetching || self.browse_loading;
        let search = self.browse_search.trim().to_lowercase();
        let imported_count = self
            .browse_articles
            .iter()
            .filter(|a| self.browse_imported_urls.contains(&a.url))
            .count();
        let new_count = self
            .browse_articles
            .iter()
            .filter(|a| !self.browse_imported_urls.contains(&a.url))
            .count();

        // Section picker
        let section_options: Vec<String> = SECTIONS.iter().map(|s| s.label.to_owned()).collect();
        let current_section_label = SECTIONS
            .iter()
            .find(|s| s.id == self.browse_section)
            .map(|s| s.label.to_owned())
            .unwrap_or_else(|| self.browse_section.clone());

        let section_pick = pick_list(section_options, Some(current_section_label), |selected| {
            let section_id = SECTIONS
                .iter()
                .find(|s| s.label == selected)
                .map(|s| s.id.to_owned())
                .unwrap_or(selected);
            Message::BrowseSectionChanged(section_id)
        })
        .width(320);

        let search_input = text_input("Filter...", &self.browse_search)
            .on_input(Message::BrowseSearchChanged)
            .width(200);

        let mut toolbar = Row::new().spacing(8).align_y(iced::Alignment::Center);
        toolbar = toolbar.push(text("Section").size(14));
        toolbar = toolbar.push(section_pick);
        toolbar = toolbar.push(search_input);
        toolbar = toolbar.push(
            text(format!("Loaded {}", self.browse_articles.len()))
                .size(13)
                .color(TEXT_SECONDARY),
        );

        // Action row: buttons + checkbox + stats, all on one line
        let mut action_row = Row::new().spacing(6).align_y(iced::Alignment::Center);
        if !browse_busy {
            action_row =
                action_row.push(button(text("Refresh").size(13)).on_press(Message::BrowseRefresh));
            action_row = action_row
                .push(button(text("All sections").size(13)).on_press(Message::BrowseAllSections));
            action_row =
                action_row.push(button(text("Find new").size(13)).on_press(Message::BrowseFindNew));
        }
        if browse_busy {
            action_row = action_row.push(text("Loading...").size(13).color(ACCENT_BLUE));
        }
        action_row = action_row.push(
            checkbox("Only new", self.browse_only_new)
                .on_toggle(Message::BrowseToggleOnlyNew)
                .size(16)
                .text_size(13),
        );
        if !browse_busy {
            action_row = action_row.push(
                button(text("Select new").size(12))
                    .style(button::secondary)
                    .on_press(Message::BrowseSelectVisibleNew),
            );
            action_row = action_row.push(
                button(text("Clear").size(12))
                    .style(button::secondary)
                    .on_press(Message::BrowseClearSelection),
            );
            if !self.browse_selected.is_empty() {
                action_row = action_row.push(
                    button(text(format!("Fetch & Save ({})", self.browse_selected.len())).size(13))
                        .on_press(Message::BrowseFetchSelected),
                );
            }
        }
        action_row = action_row.push(horizontal_space());
        action_row = action_row.push(
            text(format!(
                "{} sel, {} imported, {} new",
                self.browse_selected.len(),
                imported_count,
                new_count,
            ))
            .size(12)
            .color(TEXT_SECONDARY),
        );

        // Import progress
        let progress_el: Element<Message> = if let Some(progress) = &self.import_progress {
            let total = progress.total.unwrap_or(0);
            let fraction = if total == 0 {
                0.0
            } else {
                progress.processed as f32 / total as f32
            };
            wcolumn![
                text(format!(
                    "Importing: {} / {} (saved {}, failed {})",
                    progress.processed, total, progress.saved_count, progress.failed_count
                ))
                .size(13),
                progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0)).height(6),
            ]
            .spacing(4)
            .into()
        } else {
            Space::with_height(0).into()
        };

        // Article list
        let visible: Vec<&ArticleSummary> = self
            .browse_articles
            .iter()
            .filter(|a| {
                if self.browse_only_new && self.browse_imported_urls.contains(&a.url) {
                    return false;
                }
                if !search.is_empty() && !article_matches_search(a, &search) {
                    return false;
                }
                true
            })
            .collect();

        let mut article_list = Column::new().spacing(6);
        for article in &visible {
            article_list = article_list.push(self.view_browse_card(article, browse_busy));
        }

        let mut content = Column::new().spacing(8).padding(16).width(Length::Fill);
        content = content.push(toolbar);
        content = content.push(action_row);
        content = content.push(progress_el);
        content = content.push(scrollable(article_list).height(Length::Fill));

        // Load more button at bottom
        if !browse_busy
            && !(self.browse_scope == BrowseScope::CurrentSection && self.browse_end_reached)
        {
            content = content
                .push(button(text("Load 80 more").size(13)).on_press(Message::BrowseLoadMore));
        }

        content.into()
    }

    fn view_browse_card(&self, article: &ArticleSummary, busy: bool) -> Element<'_, Message> {
        let checked = self.browse_selected.contains(&article.url);
        let imported = self.browse_imported_urls.contains(&article.url);
        let url = article.url.clone();
        let title = article.title.clone();
        let section = article.section.clone();
        let author = article.author.clone();
        let date = article.date.clone();
        let teaser = article.teaser.clone();
        let url_display = compact_url(&url);

        let mut title_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
        title_row = title_row.push(
            checkbox("", checked)
                .on_toggle({
                    let url = url.clone();
                    move |_| Message::BrowseToggleArticle(url.clone())
                })
                .size(16),
        );
        title_row = title_row.push(text(title).size(15));

        let mut tags_row = Row::new().spacing(6).align_y(iced::Alignment::Center);
        tags_row = tags_row.push(tag_badge(section));
        if !author.is_empty() {
            tags_row = tags_row.push(tag_badge(truncate_for_ui(&author, 28)));
        }
        if !date.is_empty() {
            tags_row = tags_row.push(tag_badge(date));
        }
        if imported {
            tags_row = tags_row.push(success_badge("Imported"));
        }
        if !busy {
            tags_row = tags_row.push(
                button(text("Open original").size(12).color(LINK_BLUE))
                    .style(button::text)
                    .on_press(Message::OpenUrl(url.clone())),
            );
            tags_row = tags_row.push(
                button(text("Preview").size(12).color(LINK_BLUE))
                    .style(button::text)
                    .on_press(Message::OpenPreview(url)),
            );
        }

        let mut card = Column::new().spacing(4).width(Length::Fill);
        card = card.push(title_row);
        if !teaser.is_empty() {
            card = card.push(
                text(truncate_for_ui(&teaser, 220))
                    .size(13)
                    .color(TEXT_SECONDARY)
                    .width(Length::Fill),
            );
        }
        card = card.push(tags_row);
        card = card.push(text(url_display).size(11).color(TEXT_DIM));

        container(card)
            .padding(10)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Library view ───────────────────────────────────────────────

    fn view_library(&self) -> Element<'_, Message> {
        let library_busy = self.lingq_uploading;

        // Filter bar
        let search_input = text_input("Search...", &self.library_search)
            .on_input(Message::LibrarySearchChanged)
            .width(220);

        let topic_counts = collect_topic_counts(&self.library_articles);
        let mut topic_options = vec!["All topics".to_owned()];
        for (topic, count) in &topic_counts {
            topic_options.push(format!("{} ({})", topic, count));
        }
        let current_topic_label = if self.library_topic.is_empty() {
            "All topics".to_owned()
        } else {
            topic_counts
                .get(&self.library_topic)
                .map(|c| format!("{} ({})", self.library_topic, c))
                .unwrap_or_else(|| self.library_topic.clone())
        };
        let topic_pick = pick_list(topic_options, Some(current_topic_label), |selected| {
            if selected.starts_with("All topics") {
                Message::LibraryTopicChanged(String::new())
            } else {
                let topic = selected
                    .rsplit_once(" (")
                    .map(|(t, _)| t.to_owned())
                    .unwrap_or(selected);
                Message::LibraryTopicChanged(topic)
            }
        })
        .width(200);

        let sort_options: Vec<String> = [
            LibrarySortMode::Newest,
            LibrarySortMode::Oldest,
            LibrarySortMode::Longest,
            LibrarySortMode::Shortest,
            LibrarySortMode::Title,
        ]
        .iter()
        .map(|s| s.label().to_owned())
        .collect();

        let sort_pick = pick_list(
            sort_options,
            Some(self.library_sort_mode.label().to_owned()),
            |selected| {
                let mode = match selected.as_str() {
                    "Oldest" => LibrarySortMode::Oldest,
                    "Longest" => LibrarySortMode::Longest,
                    "Shortest" => LibrarySortMode::Shortest,
                    "Title" => LibrarySortMode::Title,
                    _ => LibrarySortMode::Newest,
                };
                Message::LibrarySortChanged(mode)
            },
        )
        .width(120);

        let toggle_filters_label = if self.library_filters_expanded {
            "Hide filters"
        } else {
            "Show filters"
        };

        let mut content = Column::new().spacing(8).padding(16).width(Length::Fill);

        content = content.push(
            row![
                text("Library Filters").size(16),
                button(text(toggle_filters_label).size(13))
                    .style(button::secondary)
                    .on_press(Message::LibraryToggleFilters),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        );

        if self.library_filters_expanded {
            let filter_row1 = row![
                text("Search").size(13),
                search_input,
                text("Topic").size(13),
                topic_pick,
                checkbox("Only not uploaded", self.library_only_not_uploaded)
                    .on_toggle(Message::LibraryToggleNotUploaded)
                    .size(16)
                    .text_size(13),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let min_input = text_input("e.g. 600", &self.library_word_count_min)
                .on_input(Message::LibraryMinWordsChanged)
                .width(80);
            let max_input = text_input("e.g. 1800", &self.library_word_count_max)
                .on_input(Message::LibraryMaxWordsChanged)
                .width(80);

            let mut filter_row2 = Row::new().spacing(8).align_y(iced::Alignment::Center);
            filter_row2 = filter_row2.push(text("Min words").size(13));
            filter_row2 = filter_row2.push(min_input);
            filter_row2 = filter_row2.push(text("Max words").size(13));
            filter_row2 = filter_row2.push(max_input);
            filter_row2 = filter_row2.push(sort_pick);
            filter_row2 = filter_row2.push(
                checkbox("Dense", self.library_dense_mode)
                    .on_toggle(Message::LibraryToggleDense)
                    .size(16)
                    .text_size(13),
            );
            filter_row2 = filter_row2.push(
                checkbox("Group by topic", self.library_group_by_topic)
                    .on_toggle(Message::LibraryToggleGroupByTopic)
                    .size(16)
                    .text_size(13),
            );
            filter_row2 = filter_row2
                .push(button(text("Refresh").size(13)).on_press(Message::LibraryRefresh));
            if self.library_loading {
                filter_row2 = filter_row2.push(text("Loading...").size(13).color(ACCENT_BLUE));
            }

            content = content.push(
                container(wcolumn![filter_row1, filter_row2].spacing(8))
                    .padding(10)
                    .width(Length::Fill)
                    .style(|_theme| container::Style {
                        background: Some(iced::Background::Color(BG_CARD)),
                        border: iced::Border {
                            color: BORDER_SUBTLE,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }

        // Get filtered articles using the cached method (we call the mutable version via a workaround)
        let display_articles = self.get_display_library_articles();
        let filtered_count = display_articles.len();

        // Selection / action bar with LingQ course inline
        let upload_sel_count = self.lingq_selected_articles.len();
        let mut action_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
        action_row = action_row.push(
            text(format!(
                "{} shown, {} selected",
                filtered_count, upload_sel_count,
            ))
            .size(13)
            .color(TEXT_SECONDARY),
        );
        if !library_busy {
            action_row = action_row.push(
                button(text("Select all").size(12))
                    .style(button::secondary)
                    .on_press(Message::LibrarySelectAllVisible),
            );
            action_row = action_row.push(
                button(text("Select not uploaded").size(12))
                    .style(button::secondary)
                    .on_press(Message::LibrarySelectAllNotUploaded),
            );
            action_row = action_row.push(
                button(text("Clear").size(12))
                    .style(button::secondary)
                    .on_press(Message::LibraryClearSelection),
            );
        }
        action_row = action_row.push(horizontal_space());
        // Inline LingQ course picker
        if self.lingq_connected {
            action_row = action_row.push(self.view_lingq_panel());
        }
        content = content.push(action_row);

        // Upload progress
        if let Some(progress) = &self.upload_progress {
            let fraction = if progress.total == 0 {
                0.0
            } else {
                progress.processed as f32 / progress.total as f32
            };
            content = content.push(
                wcolumn![
                    text(format!(
                        "Uploading: {} / {} (uploaded {}, failed {})",
                        progress.processed,
                        progress.total,
                        progress.uploaded,
                        progress.failed_count
                    ))
                    .size(13),
                    progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0)).height(6),
                ]
                .spacing(4),
            );
        }

        // Paging
        if let Some(page) = &self.library_page_cache {
            if page.total_count > page.limit {
                let current_page = (page.offset / page.limit.max(1)) + 1;
                let total_pages = page.total_count.div_ceil(page.limit.max(1));
                let mut paging_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
                paging_row =
                    paging_row.push(text(format!("Page {current_page} of {total_pages}")).size(13));
                if self.library_page_index > 0 {
                    paging_row = paging_row.push(
                        button(text("Previous").size(12))
                            .style(button::secondary)
                            .on_press(Message::LibraryPrevPage),
                    );
                }
                if current_page < total_pages {
                    paging_row = paging_row.push(
                        button(text("Next").size(12))
                            .style(button::secondary)
                            .on_press(Message::LibraryNextPage),
                    );
                }
                content = content.push(paging_row);
            }
        }

        // Article list
        let mut article_list = Column::new().spacing(4);

        if self.library_group_by_topic {
            let mut current_topic = String::new();
            for article in &display_articles {
                let topic = effective_topic_for_article(article);
                if topic != current_topic {
                    current_topic = topic.clone();
                    article_list = article_list.push(Space::with_height(6));
                    article_list = article_list.push(text(format!("{current_topic}")).size(16));
                }
                article_list = article_list.push(self.view_library_card(article, library_busy));
            }
        } else {
            for article in &display_articles {
                article_list = article_list.push(self.view_library_card(article, library_busy));
            }
        }

        content = content.push(scrollable(article_list).height(Length::Fill));

        content.into()
    }

    /// Read-only view of filtered library articles using current state.
    fn get_display_library_articles(&self) -> Vec<ArticleListItem> {
        let trimmed_search = self.library_search.trim();
        let min_words =
            parse_optional_positive_usize(&self.library_word_count_min, "Min").unwrap_or(None);
        let max_words =
            parse_optional_positive_usize(&self.library_word_count_max, "Max").unwrap_or(None);

        // If we have a page cache and it's current, use it
        if let Some(page) = &self.library_page_cache {
            return page.items.clone();
        }
        // If we have a filtered cache and it's current, use it
        if !self.library_filtered_cache_results.is_empty()
            && self.library_filtered_cache_revision == self.library_data_revision
        {
            return self.library_filtered_cache_results.clone();
        }

        // Otherwise compute from scratch (read-only path)
        let mut articles = if trimmed_search.is_empty() {
            self.library_articles.clone()
        } else if self.library_search_cache_query == trimmed_search {
            self.library_search_cache_results.clone()
        } else {
            // Can't do DB search from immutable view, fall back to in-memory
            self.library_articles
                .iter()
                .filter(|a| {
                    let s = trimmed_search.to_lowercase();
                    a.title.to_lowercase().contains(&s)
                        || a.subtitle.to_lowercase().contains(&s)
                        || a.teaser.to_lowercase().contains(&s)
                        || a.url.to_lowercase().contains(&s)
                })
                .cloned()
                .collect()
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

        articles
    }

    fn view_library_card(&self, article: &ArticleListItem, busy: bool) -> Element<'_, Message> {
        let selected = self.lingq_selected_articles.contains(&article.id);
        let id = article.id;
        let title = article.title.clone();
        let section = article.section.clone();
        let word_count = article.word_count;
        let uploaded = article.uploaded_to_lingq;
        let dense = self.library_dense_mode;
        let preview_line = library_card_preview_line(article);

        let mut title_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
        if !busy {
            title_row = title_row.push(
                checkbox("", selected)
                    .on_toggle(move |_| Message::LibraryToggleArticle(id))
                    .size(16),
            );
        }
        title_row = title_row.push(text(title).size(14));

        let mut tags_row = Row::new().spacing(6).align_y(iced::Alignment::Center);
        tags_row = tags_row.push(tag_badge(section));
        tags_row = tags_row.push(text(format!("{} w", word_count)).size(11).color(TEXT_DIM));
        if uploaded {
            tags_row = tags_row.push(success_badge("Uploaded"));
        }
        if !busy {
            tags_row = tags_row.push(
                button(text("Preview").size(11).color(LINK_BLUE))
                    .style(button::text)
                    .on_press(Message::OpenLibraryPreview(id)),
            );
            tags_row = tags_row.push(
                button(text("Open").size(11).color(LINK_BLUE))
                    .style(button::text)
                    .on_press(Message::OpenArticle(id)),
            );
            tags_row = tags_row.push(
                button(text("Delete").size(11).color(ACCENT_RED))
                    .style(button::text)
                    .on_press(Message::LibraryDeleteArticle(id)),
            );
        }

        let mut card = Column::new().spacing(3).width(Length::Fill);
        card = card.push(title_row);
        if !dense && !preview_line.is_empty() {
            card = card.push(
                text(preview_line)
                    .size(12)
                    .color(TEXT_SECONDARY)
                    .width(Length::Fill),
            );
        }
        card = card.push(tags_row);

        container(card)
            .padding(if dense { 4 } else { 8 })
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── LingQ upload panel ─────────────────────────────────────────

    fn view_lingq_panel(&self) -> Element<'_, Message> {
        if !self.lingq_connected {
            return Space::with_height(0).into();
        }

        // Build label-to-id mapping for the collection pick list
        let mut label_to_id: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let collection_options: Vec<String> = std::iter::once("Standalone lesson".to_owned())
            .chain(self.lingq_collections.iter().map(|c| {
                let label = format!("{} ({})", c.title, c.lessons_count);
                label_to_id.insert(label.clone(), c.id);
                label
            }))
            .collect();

        let current_collection_label = self
            .lingq_selected_collection
            .and_then(|id| {
                self.lingq_collections
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| format!("{} ({})", c.title, c.lessons_count))
            })
            .unwrap_or_else(|| "Standalone lesson".to_owned());

        let collection_pick = pick_list(
            collection_options,
            Some(current_collection_label),
            move |selected| {
                if selected == "Standalone lesson" {
                    Message::LingqCollectionChanged(None)
                } else {
                    let id = label_to_id.get(&selected).copied();
                    Message::LingqCollectionChanged(id)
                }
            },
        )
        .width(260);

        // Single row: Course picker + refresh + upload controls
        let mut panel_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
        panel_row = panel_row.push(text("Course").size(13));
        panel_row = panel_row.push(collection_pick);
        panel_row = panel_row.push(
            button(text("Refresh courses").size(12))
                .style(button::secondary)
                .on_press(Message::LingqRefreshCollections),
        );
        panel_row = panel_row.push(
            button(text("Clear upload").size(12))
                .style(button::secondary)
                .on_press(Message::LingqClearUploadSelection),
        );
        if !self.lingq_uploading && !self.lingq_selected_articles.is_empty() {
            panel_row = panel_row.push(
                button(text(format!("Upload {}", self.lingq_selected_articles.len())).size(13))
                    .on_press(Message::LingqUploadSelected),
            );
        }
        if self.lingq_uploading {
            panel_row = panel_row.push(text("Uploading...").size(13).color(ACCENT_BLUE));
        }

        panel_row.into()
    }

    // ── Article detail view ────────────────────────────────────────

    fn view_article(&self) -> Element<'_, Message> {
        let Some(article) = &self.article_detail else {
            return wcolumn![
                button(text("Back to Library").size(14)).on_press(Message::ArticleBack),
                text("No article selected.").size(14),
            ]
            .spacing(8)
            .padding(16)
            .into();
        };

        let toolbar = row![
            button(text("Back").size(14))
                .style(button::secondary)
                .on_press(Message::ArticleBack),
            button(text("Copy Text").size(14))
                .style(button::secondary)
                .on_press(Message::ArticleCopyText),
            button(text("View original").size(14))
                .style(button::secondary)
                .on_press(Message::OpenUrl(article.url.clone())),
        ]
        .spacing(8);

        let mut header = Column::new().spacing(6);
        header = header.push(text(&article.title).size(22));
        if !article.subtitle.is_empty() {
            header = header.push(text(&article.subtitle).size(17).color(TEXT_SECONDARY));
        }

        let mut meta_row = Row::new().spacing(8);
        if !article.author.is_empty() {
            meta_row = meta_row.push(tag_badge(format!("Von {}", article.author)));
        }
        if !article.date.is_empty() {
            meta_row = meta_row.push(tag_badge(article.date.clone()));
        }
        meta_row = meta_row.push(tag_badge(effective_topic_for_article(article)));
        meta_row = meta_row.push(tag_badge(format!("{} words", article.word_count)));
        header = header.push(meta_row);
        header = header.push(horizontal_rule(1));

        // Body text
        let mut body = Column::new().spacing(8);
        for block in article.body_text.split("\n\n") {
            if let Some(heading) = block.strip_prefix("## ") {
                body = body.push(Space::with_height(4));
                body = body.push(text(heading).size(20));
            } else if !block.trim().is_empty() {
                body = body.push(text(block).size(14));
            }
        }

        wcolumn![
            toolbar,
            Space::with_height(8),
            container(wcolumn![header, scrollable(body).height(Length::Fill)].spacing(8))
                .padding(16)
                .width(Length::Fill)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(BG_CARD)),
                    border: iced::Border {
                        color: BORDER_SUBTLE,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // ── Preview drawer ─────────────────────────────────────────────

    fn view_preview_drawer(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(8).padding(16).width(400);

        col = col.push(
            row![
                text("Preview").size(18),
                horizontal_space(),
                button(text("Close").size(13))
                    .style(button::secondary)
                    .on_press(Message::ClosePreview),
            ]
            .align_y(iced::Alignment::Center),
        );
        col = col.push(horizontal_rule(1));

        if self.preview_loading {
            col = col.push(text("Fetching article...").size(14).color(TEXT_DIM));
            return container(col)
                .height(Length::Fill)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(BG_CARD)),
                    ..Default::default()
                })
                .into();
        }

        let Some(article) = &self.preview_article else {
            col = col.push(text("No preview available.").size(14));
            return container(col)
                .height(Length::Fill)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(BG_CARD)),
                    ..Default::default()
                })
                .into();
        };

        col = col.push(text(&article.title).size(18));
        if !article.subtitle.is_empty() {
            col = col.push(text(&article.subtitle).size(14).color(TEXT_SECONDARY));
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

        let mut meta_row = Row::new().spacing(6);
        if !article.author.is_empty() {
            meta_row = meta_row.push(tag_badge(format!("Von {}", article.author)));
        }
        if !article.date.is_empty() {
            meta_row = meta_row.push(tag_badge(article.date.clone()));
        }
        meta_row = meta_row.push(tag_badge(preview_topic.clone()));
        meta_row = meta_row.push(tag_badge(format!("{} w", article.word_count)));
        col = col.push(meta_row);

        col = col.push(text(compact_url(&article.url)).size(11).color(TEXT_DIM));

        let mut action_row = Row::new().spacing(8);
        if let Some(stored) = &self.preview_stored_article {
            action_row = action_row.push(
                button(text("Open full article").size(12))
                    .on_press(Message::OpenFullArticle(stored.id)),
            );
        }
        action_row = action_row.push(
            button(text("Original").size(12).color(LINK_BLUE))
                .style(button::text)
                .on_press(Message::OpenUrl(article.url.clone())),
        );
        col = col.push(action_row);

        col = col.push(horizontal_rule(1));
        col = col.push(text("Quick preview").size(14));
        col = col.push(text(preview_excerpt(&article.body_text, 2, 900)).size(13));

        // Full text in scrollable
        let mut body = Column::new().spacing(6);
        for block in article.body_text.split("\n\n") {
            if let Some(heading) = block.strip_prefix("## ") {
                body = body.push(text(heading).size(16));
            } else if !block.trim().is_empty() {
                body = body.push(text(block).size(13));
            }
        }
        col = col.push(scrollable(body).height(Length::Fill));

        container(col)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Diagnostics view ───────────────────────────────────────────

    fn view_diagnostics(&self) -> Element<'_, Message> {
        let data_dir = app_paths::data_dir().ok();
        let log_path = app_paths::app_log_path().ok();
        let exe_path = std::env::current_exe().ok();

        let credential_status = credential_store::load_lingq_api_key()
            .ok()
            .flatten()
            .map(|_| "available")
            .unwrap_or("not found");

        let perf = crate::perf::snapshot();

        let info_row = row![
            text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(13),
            text(format!("Credential Manager: {credential_status}")).size(13),
            text(format!(
                "Browse cache H/M: {}/{}",
                perf.browse_cache_hits, perf.browse_cache_misses
            ))
            .size(13),
        ]
        .spacing(16);

        let mut btn_row = Row::new().spacing(6);
        btn_row = btn_row.push(
            button(text("Open data folder").size(12))
                .style(button::secondary)
                .on_press(Message::OpenDataFolder),
        );
        btn_row = btn_row.push(
            button(text("Open log file").size(12))
                .style(button::secondary)
                .on_press(Message::OpenLogFile),
        );
        btn_row = btn_row.push(
            button(text("Copy recent log").size(12))
                .style(button::secondary)
                .on_press(Message::CopyRecentLog),
        );
        btn_row = btn_row.push(
            button(text("Support bundle").size(12))
                .style(button::secondary)
                .on_press(Message::CreateSupportBundle),
        );
        btn_row = btn_row.push(
            button(text("Clear browse cache").size(12))
                .style(button::secondary)
                .on_press(Message::ClearBrowseCache),
        );
        btn_row = btn_row.push(
            button(text("Compact DB").size(12))
                .style(button::secondary)
                .on_press(Message::CompactLocalData),
        );
        btn_row = btn_row.push(
            button(text("Rebuild index").size(12))
                .style(button::secondary)
                .on_press(Message::RebuildSearchIndex),
        );
        btn_row = btn_row.push(
            button(text("Verify DB").size(12))
                .style(button::secondary)
                .on_press(Message::VerifyDatabase),
        );

        let mut paths_col = Column::new().spacing(2);
        if let Some(path) = &data_dir {
            paths_col = paths_col.push(
                text(format!("Data: {}", path.display()))
                    .size(11)
                    .color(TEXT_DIM),
            );
        }
        if let Some(path) = &log_path {
            paths_col = paths_col.push(
                text(format!("Log: {}", path.display()))
                    .size(11)
                    .color(TEXT_DIM),
            );
        }
        if let Some(path) = &exe_path {
            paths_col = paths_col.push(
                text(format!("Exe: {}", path.display()))
                    .size(11)
                    .color(TEXT_DIM),
            );
        }

        let info_panel = container(wcolumn![info_row, btn_row, paths_col].spacing(8))
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Jobs panel
        let jobs_panel = self.view_jobs_panel();

        // Failures panel
        let failures_panel = self.view_failures_panel();

        // Recent log
        let log_panel = self.view_log_panel();

        let mut content = Column::new().spacing(10).padding(16).width(Length::Fill);
        content = content.push(text("Diagnostics").size(20));
        content = content.push(info_panel);
        content = content.push(jobs_panel);
        content = content.push(failures_panel);
        content = content.push(log_panel);

        scrollable(content).height(Length::Fill).into()
    }

    fn view_jobs_panel(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(6);
        col = col.push(text("Jobs").size(15));

        // Active job
        if let Some(job) = &self.active_job {
            let fraction = if job.total == 0 {
                0.0
            } else {
                job.processed as f32 / job.total as f32
            };
            col = col.push(text(&job.label).size(14));
            col = col.push(progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0)).height(8));
            col = col.push(
                text(format!(
                    "{}/{} — success {}, failed {}",
                    job.processed, job.total, job.succeeded, job.failed
                ))
                .size(12)
                .color(TEXT_SECONDARY),
            );
            col = col.push(
                button(text("Cancel").size(12))
                    .style(button::secondary)
                    .on_press(Message::CancelActiveJob),
            );
        } else {
            col = col.push(text("No active job.").size(12).color(TEXT_DIM));
        }

        // Queue controls
        let mut queue_row = Row::new().spacing(6);
        queue_row = queue_row.push(
            text(format!("{} queued", self.queued_jobs.len()))
                .size(12)
                .color(TEXT_SECONDARY),
        );
        if self.queue_paused {
            queue_row = queue_row.push(
                button(text("Resume").size(12))
                    .style(button::secondary)
                    .on_press(Message::ResumeQueue),
            );
        } else {
            queue_row = queue_row.push(
                button(text("Pause").size(12))
                    .style(button::secondary)
                    .on_press(Message::PauseQueue),
            );
        }
        if !self.queued_jobs.is_empty() {
            queue_row = queue_row.push(
                button(text("Clear queue").size(12))
                    .style(button::secondary)
                    .on_press(Message::ClearQueuedJobs),
            );
            if self.active_job.is_none() {
                queue_row = queue_row.push(
                    button(text("Run upload now").size(12)).on_press(Message::RunQueuedUploadNow),
                );
            }
        }
        col = col.push(queue_row);

        // Completed jobs
        col = col.push(Space::with_height(4));
        col = col.push(text("History").size(14));
        for job in self.completed_jobs.iter().take(10) {
            let selected = self
                .diagnostics_selected_job_id
                .is_some_and(|id| id == job.id);
            let status_color = if job.success {
                ACCENT_GREEN
            } else {
                ACCENT_RED
            };
            let mut job_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
            job_row = job_row.push(
                text(if job.success { "✓" } else { "✗" })
                    .size(13)
                    .color(status_color),
            );
            job_row = job_row.push(
                button(text(&job.label).size(12))
                    .style(if selected {
                        button::primary
                    } else {
                        button::text
                    })
                    .on_press(Message::SelectDiagnosticsJob(job.id)),
            );
            job_row = job_row.push(
                text(format_job_timestamp(&job.recorded_at))
                    .size(11)
                    .color(TEXT_DIM),
            );
            col = col.push(job_row);

            if selected {
                col = col.push(text(&job.summary).size(12).color(TEXT_SECONDARY));
            }
        }

        container(col)
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_failures_panel(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4);
        col = col.push(text("Failed Items").size(15));

        let mut fail_row = Row::new().spacing(6);
        if !self.failed_fetches.is_empty() {
            fail_row = fail_row.push(
                button(text("Retry imports").size(12))
                    .style(button::secondary)
                    .on_press(Message::RetryFailedImports),
            );
        }
        if !self.last_failed_uploads.is_empty() {
            fail_row = fail_row.push(
                button(text("Retry uploads").size(12))
                    .style(button::secondary)
                    .on_press(Message::RetryFailedUploads),
            );
        }
        col = col.push(fail_row);

        if self.failed_fetches.is_empty() && self.last_failed_uploads.is_empty() {
            col = col.push(text("No failures recorded.").size(12).color(TEXT_DIM));
        }
        for item in self.failed_fetches.iter().take(10) {
            col = col.push(
                text(format!(
                    "[import] {}: {}",
                    truncate_for_ui(
                        if item.title.is_empty() {
                            &item.url
                        } else {
                            &item.title
                        },
                        40
                    ),
                    truncate_for_ui(&item.message, 60)
                ))
                .size(11)
                .color(TEXT_DIM),
            );
        }
        for item in self.last_failed_uploads.iter().take(10) {
            col = col.push(
                text(format!(
                    "[upload] {}: {}",
                    truncate_for_ui(&item.title, 40),
                    truncate_for_ui(&item.message, 60)
                ))
                .size(11)
                .color(TEXT_DIM),
            );
        }

        // Task failures
        if !self.recent_task_failures.is_empty() {
            col = col.push(Space::with_height(4));
            col = col.push(
                row![
                    text("Task Errors").size(14),
                    button(text("Clear").size(11))
                        .style(button::secondary)
                        .on_press(Message::ClearTaskFailures),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
            for err in self.recent_task_failures.iter().take(8) {
                col = col.push(
                    text(truncate_for_ui(&err.notice_message(), 120))
                        .size(11)
                        .color(ACCENT_RED),
                );
            }
        }

        container(col)
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_log_panel(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4);
        col = col.push(text("Recent Log").size(15));
        match read_recent_log_excerpt(18) {
            Ok(log_text) => {
                col = col.push(text(log_text).size(11).color(TEXT_DIM));
            }
            Err(err) => {
                col = col.push(text(err).size(11).color(ACCENT_RED));
            }
        }

        container(col)
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CARD)),
                border: iced::Border {
                    color: BORDER_SUBTLE,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── LingQ settings (modal-like view) ───────────────────────────

    fn view_lingq_settings(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(12).padding(24).width(Length::Fill);

        col = col.push(
            row![
                text("LingQ Settings").size(22),
                horizontal_space(),
                button(text("Close").size(14))
                    .style(button::secondary)
                    .on_press(Message::ToggleLingqSettings),
            ]
            .align_y(iced::Alignment::Center),
        );

        col = col.push(
            text("Manage your LingQ login or token here.")
                .size(14)
                .color(TEXT_SECONDARY),
        );

        // Auth mode tabs
        let account_btn = if self.lingq_auth_mode == LingqAuthMode::Account {
            button(text("Account Login").size(13)).style(button::primary)
        } else {
            button(text("Account Login").size(13)).style(button::secondary)
        };
        let token_btn = if self.lingq_auth_mode == LingqAuthMode::Token {
            button(text("Token / API Key").size(13)).style(button::primary)
        } else {
            button(text("Token / API Key").size(13)).style(button::secondary)
        };
        col = col.push(
            row![
                account_btn.on_press(Message::LingqAuthModeChanged(LingqAuthMode::Account)),
                token_btn.on_press(Message::LingqAuthModeChanged(LingqAuthMode::Token)),
            ]
            .spacing(8),
        );

        if self.lingq_auth_mode == LingqAuthMode::Account {
            let username_input = text_input("Username or email", &self.lingq_username)
                .on_input(Message::LingqUsernameChanged)
                .width(220);
            let password_input = text_input("Password", &self.lingq_password)
                .on_input(Message::LingqPasswordChanged)
                .secure(true)
                .width(180);

            let mut login_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
            login_row = login_row.push(text("Username").size(13));
            login_row = login_row.push(username_input);
            login_row = login_row.push(text("Password").size(13));
            login_row = login_row.push(password_input);
            login_row =
                login_row.push(button(text("Sign in").size(13)).on_press(Message::LingqSignIn));
            if self.lingq_loading_collections {
                login_row = login_row.push(text("Loading...").size(13).color(ACCENT_BLUE));
            }

            col = col.push(login_row);
            col = col.push(
                text("Signs in, retrieves your token, stores it in Windows Credential Manager.")
                    .size(12)
                    .color(TEXT_DIM),
            );
        }

        // Token input (always shown)
        let token_input = text_input("API key / token", &self.lingq_api_key)
            .on_input(Message::LingqApiKeyChanged)
            .secure(true)
            .width(320);

        let mut token_row = Row::new().spacing(8).align_y(iced::Alignment::Center);
        token_row = token_row.push(text("Token").size(13));
        token_row = token_row.push(token_input);
        token_row =
            token_row.push(button(text("Connect").size(13)).on_press(Message::LingqConnect));
        token_row = token_row.push(
            button(text("Disconnect").size(13))
                .style(button::secondary)
                .on_press(Message::LingqDisconnect),
        );
        if self.lingq_loading_collections {
            token_row = token_row.push(text("Loading...").size(13).color(ACCENT_BLUE));
        }
        col = col.push(token_row);
        col = col.push(
            text("Token stored in Windows Credential Manager.")
                .size(12)
                .color(TEXT_DIM),
        );

        // Connection status
        let status_row = row![
            text(if self.lingq_connected {
                "Status: Connected"
            } else {
                "Status: Not connected"
            })
            .size(14)
            .color(if self.lingq_connected {
                ACCENT_GREEN
            } else {
                ACCENT_RED
            }),
            button(text("Test / refresh courses").size(13))
                .style(button::secondary)
                .on_press(Message::LingqRefreshCollections),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center);
        col = col.push(status_row);

        col.into()
    }
}

// ── Reusable view helpers ──────────────────────────────────────────

fn sidebar_stat(label: &str, value: i64) -> Element<'_, Message> {
    row![
        text(format!("{label}:")).size(12).color(TEXT_SECONDARY),
        text(format!("{value}")).size(12),
    ]
    .spacing(6)
    .into()
}

fn tag_badge(label: impl Into<String>) -> Element<'static, Message> {
    let label: String = label.into();
    container(text(label).size(11).color(TEXT_SECONDARY))
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_TAG)),
            border: iced::Border {
                color: BORDER_SUBTLE,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn success_badge(label: impl Into<String>) -> Element<'static, Message> {
    let label: String = label.into();
    container(text(label).size(11).color(ACCENT_GREEN))
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_TAG)),
            border: iced::Border {
                color: ACCENT_GREEN,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}
