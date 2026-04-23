use super::*;

impl SoziopolisLingqGui {
    pub(super) fn guard_ui_phase(&mut self, phase: &str, run: impl FnOnce(&mut Self)) {
        if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| run(self))) {
            self.recover_from_ui_panic(phase, payload);
        }
    }

    pub(super) fn recover_from_ui_panic(&mut self, phase: &str, payload: Box<dyn Any + Send>) {
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

    pub(super) fn persist_lingq_api_key(&mut self) -> bool {
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

    pub(super) fn clear_stored_lingq_api_key(&mut self) -> bool {
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

    pub(super) fn browse_article_passes_new_filter(&self, article: &ArticleSummary) -> bool {
        !self.browse_imported_urls.contains(&article.url)
    }

    pub(super) fn browse_article_is_visible(&self, article: &ArticleSummary) -> bool {
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

    pub(super) fn filtered_library_articles(&self) -> Result<Vec<StoredArticle>, String> {
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
}

pub(super) fn render_library_article_card(
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

pub(super) fn truncate_for_ui(value: &str, max_chars: usize) -> String {
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

pub(super) fn compact_url(url: &str) -> String {
    let cleaned = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    truncate_for_ui(cleaned, 72)
}

pub(super) fn compare_library_articles(
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

pub(super) fn trim_chars_for_ui(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

pub(super) fn collect_topic_counts(articles: &[StoredArticle]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for article in articles {
        *counts
            .entry(effective_topic_for_article(article))
            .or_insert(0) += 1;
    }
    counts
}

pub(super) fn auto_topic_for_article(article: &StoredArticle) -> String {
    generated_topic_from_fields(
        &article.title,
        &article.subtitle,
        &article.section,
        &article.url,
    )
}

pub(super) fn effective_topic_for_article(article: &StoredArticle) -> String {
    if !article.custom_topic.trim().is_empty() {
        return article.custom_topic.trim().to_owned();
    }

    auto_topic_for_article(article)
}

pub(super) fn library_card_preview_line(article: &StoredArticle) -> String {
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

pub(super) fn preview_excerpt(body_text: &str, max_blocks: usize, max_chars: usize) -> String {
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

pub(super) fn clean_preview_block(block: &str) -> String {
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

pub(super) fn stored_article_to_preview_article(article: &StoredArticle) -> Article {
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

pub(super) fn latest_saved_article_date(articles: &[StoredArticle]) -> Option<NaiveDate> {
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

pub(super) fn parse_article_date(value: &str) -> Option<NaiveDate> {
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

pub(super) fn format_naive_date(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

pub(super) fn create_support_bundle(app: &SoziopolisLingqGui) -> Result<PathBuf, String> {
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

pub(super) fn load_lingq_api_key_from_storage() -> (String, Option<String>) {
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

pub(super) fn parse_positive_usize_input(value: &str, label: &str) -> Result<usize, String> {
    let trimmed = value.trim();
    let parsed = trimmed
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a positive whole number."))?;
    if parsed == 0 {
        return Err(format!("{label} must be greater than zero."));
    }
    Ok(parsed)
}

pub(super) fn job_timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

pub(super) fn format_job_timestamp(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "Unknown time".to_owned();
    }

    if let Ok(epoch_seconds) = trimmed.parse::<i64>() {
        if let Some(timestamp) = chrono::DateTime::from_timestamp(epoch_seconds, 0) {
            return timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string();
        }
    }

    trimmed.to_owned()
}

pub(super) fn parse_optional_positive_usize_input(
    value: &str,
    label: &str,
) -> Result<Option<usize>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    parse_positive_usize_input(trimmed, label).map(Some)
}

pub(super) fn configure_theme(ctx: &Context) {
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

pub(super) fn framed_panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    Frame::default()
        .fill(Color32::from_rgb(24, 28, 37))
        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 56, 74)))
        .corner_radius(12.0)
        .inner_margin(Margin::same(16))
        .show(ui, add_contents);
}

pub(super) fn article_card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
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

pub(super) fn render_import_progress(ui: &mut egui::Ui, progress: &ImportProgress) {
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

pub(super) fn format_import_progress_details(progress: &ImportProgress) -> String {
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

pub(super) fn format_import_result_summary(
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

pub(super) fn tag(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Color32::from_rgb(200, 206, 219))
            .background_color(Color32::from_rgb(33, 39, 52)),
    );
}

pub(super) fn success_tag(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Color32::from_rgb(96, 220, 137))
            .background_color(Color32::from_rgb(27, 53, 40)),
    );
}

pub(super) fn sidebar_stat_row(ui: &mut egui::Ui, label: &str, value: i64) {
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

pub(super) fn open_path_in_explorer(path: &std::path::Path) -> Result<(), String> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {}", path.display(), err))
}

pub(super) fn open_log_in_notepad(path: &std::path::Path) -> Result<(), String> {
    Command::new("notepad")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {} in Notepad: {}", path.display(), err))
}

pub(super) fn read_recent_log_excerpt(max_lines: usize) -> Result<String, String> {
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

pub(super) fn build_content_refresh_event(request_id: u64, reason: String) -> AppEvent {
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

pub(super) fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
