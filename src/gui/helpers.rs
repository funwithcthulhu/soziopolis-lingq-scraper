use super::*;

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

pub(super) fn truncate_for_ui(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        value.to_owned()
    } else {
        let trimmed: String = value.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{trimmed}...")
    }
}

pub(super) fn compact_url(url: &str) -> String {
    let cleaned = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    truncate_for_ui(cleaned, 72)
}

pub(super) trait LibraryArticleLike {
    fn url(&self) -> &str;
    fn title(&self) -> &str;
    fn subtitle(&self) -> &str;
    fn teaser(&self) -> &str;
    fn preview_summary(&self) -> &str;
    fn published_at(&self) -> &str;
    fn section(&self) -> &str;
    fn word_count(&self) -> i64;
    fn fetched_at(&self) -> &str;
    fn custom_topic(&self) -> &str;
    fn body_text(&self) -> Option<&str>;
}

impl LibraryArticleLike for ArticleListItem {
    fn url(&self) -> &str {
        &self.url
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn subtitle(&self) -> &str {
        &self.subtitle
    }
    fn teaser(&self) -> &str {
        &self.teaser
    }
    fn preview_summary(&self) -> &str {
        &self.preview_summary
    }
    fn published_at(&self) -> &str {
        &self.published_at
    }
    fn section(&self) -> &str {
        &self.section
    }
    fn word_count(&self) -> i64 {
        self.word_count
    }
    fn fetched_at(&self) -> &str {
        &self.fetched_at
    }
    fn custom_topic(&self) -> &str {
        &self.custom_topic
    }
    fn body_text(&self) -> Option<&str> {
        None
    }
}

impl LibraryArticleLike for StoredArticle {
    fn url(&self) -> &str {
        &self.url
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn subtitle(&self) -> &str {
        &self.subtitle
    }
    fn teaser(&self) -> &str {
        &self.teaser
    }
    fn preview_summary(&self) -> &str {
        &self.preview_summary
    }
    fn published_at(&self) -> &str {
        &self.published_at
    }
    fn section(&self) -> &str {
        &self.section
    }
    fn word_count(&self) -> i64 {
        self.word_count
    }
    fn fetched_at(&self) -> &str {
        &self.fetched_at
    }
    fn custom_topic(&self) -> &str {
        &self.custom_topic
    }
    fn body_text(&self) -> Option<&str> {
        Some(&self.body_text)
    }
}

pub(super) fn auto_topic_for_article(article: &impl LibraryArticleLike) -> String {
    generated_topic_from_fields(
        article.title(),
        article.subtitle(),
        article.section(),
        article.url(),
    )
}

pub(super) fn effective_topic_for_article(article: &impl LibraryArticleLike) -> String {
    if !article.custom_topic().trim().is_empty() {
        return article.custom_topic().trim().to_owned();
    }
    auto_topic_for_article(article)
}

pub(super) fn library_card_preview_line(article: &impl LibraryArticleLike) -> String {
    if !article.preview_summary().trim().is_empty() {
        return truncate_for_ui(article.preview_summary().trim(), 160);
    }
    if !article.teaser().trim().is_empty() {
        return truncate_for_ui(article.teaser().trim(), 160);
    }
    if !article.subtitle().trim().is_empty() {
        return truncate_for_ui(article.subtitle().trim(), 160);
    }
    article
        .body_text()
        .map(|body| preview_excerpt(body, 1, 160))
        .filter(|p| !p.is_empty())
        .unwrap_or_default()
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

pub(super) fn compare_library_articles(
    a: &impl LibraryArticleLike,
    b: &impl LibraryArticleLike,
    sort_mode: LibrarySortMode,
) -> std::cmp::Ordering {
    match sort_mode {
        LibrarySortMode::Newest => b
            .published_at()
            .cmp(a.published_at())
            .then_with(|| b.fetched_at().cmp(a.fetched_at())),
        LibrarySortMode::Oldest => a
            .published_at()
            .cmp(b.published_at())
            .then_with(|| a.fetched_at().cmp(b.fetched_at())),
        LibrarySortMode::Longest => b.word_count().cmp(&a.word_count()),
        LibrarySortMode::Shortest => a.word_count().cmp(&b.word_count()),
        LibrarySortMode::Title => a.title().to_lowercase().cmp(&b.title().to_lowercase()),
    }
}

pub(super) fn collect_topic_counts(articles: &[ArticleListItem]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for article in articles {
        *counts
            .entry(effective_topic_for_article(article))
            .or_insert(0) += 1;
    }
    counts
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

pub(super) fn compute_local_library_stats(articles: &[ArticleListItem]) -> LibraryStats {
    let total_articles = articles.len() as i64;
    let uploaded_articles = articles.iter().filter(|a| a.uploaded_to_lingq).count() as i64;
    let total_words: i64 = articles.iter().map(|a| a.word_count).sum();
    let average_word_count = if total_articles == 0 {
        0
    } else {
        (total_words as f64 / total_articles as f64).round() as i64
    };

    let mut section_counts = BTreeMap::new();
    for article in articles {
        let section = if article.section.trim().is_empty() {
            "Unsorted".to_owned()
        } else {
            article.section.clone()
        };
        *section_counts.entry(section).or_insert(0i64) += 1;
    }

    let mut sections: Vec<SectionCount> = section_counts
        .into_iter()
        .map(|(section, count)| SectionCount { section, count })
        .collect();
    sections.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.section.cmp(&b.section))
    });

    LibraryStats {
        total_articles,
        uploaded_articles,
        average_word_count,
        sections,
    }
}

pub(super) fn load_lingq_api_key_from_storage() -> (String, Option<String>) {
    match credential_store::load_lingq_api_key() {
        Ok(Some(api_key)) => (api_key, None),
        Ok(None) => (String::new(), None),
        Err(err) => (
            String::new(),
            Some(format!("Could not access Credential Manager: {err}")),
        ),
    }
}

pub(super) fn parse_optional_positive_usize(
    value: &str,
    label: &str,
) -> Result<Option<usize>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a positive number."))?;
    if parsed == 0 {
        return Err(format!("{label} must be greater than zero."));
    }
    Ok(Some(parsed))
}

pub(super) fn job_timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

pub(super) fn format_job_timestamp(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "Unknown time".to_owned();
    }
    if let Ok(epoch) = trimmed.parse::<i64>() {
        if let Some(ts) = chrono::DateTime::from_timestamp(epoch, 0) {
            return ts.format("%Y-%m-%d %H:%M:%S UTC").to_string();
        }
    }
    trimmed.to_owned()
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
    let mut details = Vec::new();
    if skipped_existing > 0 {
        details.push(format!("skipped {skipped_existing} already imported"));
    }
    if skipped_out_of_range > 0 {
        details.push(format!(
            "skipped {skipped_out_of_range} outside date window"
        ));
    }
    if failed_count > 0 {
        details.push(format!("{failed_count} failed"));
    }
    if !details.is_empty() {
        segments.push(format!("Also {}.", details.join(", ")));
    }
    if let Some(err) = first_error {
        segments.push(format!("First error: {err}"));
    }
    segments.join(" ")
}

pub(super) fn open_path_in_explorer(path: &std::path::Path) -> Result<(), String> {
    SysCommand::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {err}", path.display()))
}

pub(super) fn open_log_in_notepad(path: &std::path::Path) -> Result<(), String> {
    SysCommand::new("notepad")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {err}", path.display()))
}

pub(super) fn read_recent_log_excerpt(max_lines: usize) -> Result<String, String> {
    let path = app_paths::app_log_path().map_err(|e| e.to_string())?;
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let excerpt = lines[start..].join("\n");
    if excerpt.trim().is_empty() {
        Ok("Log is currently empty.".to_owned())
    } else {
        Ok(excerpt)
    }
}
