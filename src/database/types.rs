use super::*;

#[derive(Debug, Clone)]
pub struct StoredArticle {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub subtitle: String,
    pub teaser: String,
    pub preview_summary: String,
    pub author: String,
    pub date: String,
    pub published_at: String,
    pub section: String,
    pub source_kind: String,
    pub source_label: String,
    pub content_fingerprint: String,
    pub body_text: String,
    pub clean_text: String,
    pub word_count: i64,
    pub fetched_at: String,
    pub custom_topic: String,
    pub uploaded_to_lingq: bool,
    pub lingq_lesson_id: Option<i64>,
    pub lingq_lesson_url: String,
}

#[derive(Debug, Clone)]
pub struct SectionCount {
    pub section: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct LibraryStats {
    pub total_articles: i64,
    pub uploaded_articles: i64,
    pub average_word_count: i64,
    pub sections: Vec<SectionCount>,
}

pub(super) fn map_article_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredArticle> {
    let title: String = row.get(2)?;
    let subtitle: String = row.get(3)?;
    let teaser: String = row.get(4)?;
    let preview_summary: String = row.get(5)?;
    let author: String = row.get(6)?;
    let date: String = row.get(7)?;
    let published_at: String = row.get(8)?;
    let section: String = row.get(9)?;
    let source_kind: String = row.get(10)?;
    let source_label: String = row.get(11)?;
    let content_fingerprint: String = row.get(12)?;
    let body_text: String = row.get(13)?;
    Ok(StoredArticle {
        id: row.get(0)?,
        url: row.get(1)?,
        title: title.clone(),
        subtitle: subtitle.clone(),
        teaser,
        preview_summary,
        author: author.clone(),
        date: date.clone(),
        published_at,
        section,
        source_kind,
        source_label,
        content_fingerprint,
        body_text: body_text.clone(),
        clean_text: build_clean_text(&title, &subtitle, &author, &date, &body_text),
        word_count: row.get(14)?,
        fetched_at: row.get(15)?,
        custom_topic: row.get(16)?,
        uploaded_to_lingq: row.get::<_, i64>(17)? != 0,
        lingq_lesson_id: row.get(18)?,
        lingq_lesson_url: row.get(19)?,
    })
}

pub(super) fn map_article_card_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArticleListItem> {
    Ok(ArticleListItem {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        subtitle: row.get(3)?,
        teaser: row.get(4)?,
        preview_summary: row.get(5)?,
        author: row.get(6)?,
        date: row.get(7)?,
        published_at: row.get(8)?,
        section: row.get(9)?,
        word_count: row.get(10)?,
        fetched_at: row.get(11)?,
        custom_topic: row.get(12)?,
        uploaded_to_lingq: row.get::<_, i64>(13)? != 0,
        lingq_lesson_id: row.get(14)?,
        lingq_lesson_url: row.get(15)?,
    })
}

pub(super) fn build_article_preview_summary(article: &Article) -> String {
    build_preview_summary_from_fields(&article.teaser, &article.subtitle, &article.body_text)
}

pub(super) fn build_article_fingerprint(article: &Article) -> String {
    build_text_fingerprint(
        &article.title,
        &article.subtitle,
        &article.author,
        &article.date,
        &article.body_text,
    )
}

pub fn debug_article_fingerprint(article: &Article) -> String {
    build_article_fingerprint(article)
}

pub(super) fn build_text_fingerprint(
    title: &str,
    subtitle: &str,
    author: &str,
    date: &str,
    body_text: &str,
) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let body_prefix = normalize_preview_text(body_text, 320);
    let seed = format!(
        "{}|{}|{}|{}|{}",
        title.trim().to_lowercase(),
        subtitle.trim().to_lowercase(),
        author.trim().to_lowercase(),
        date.trim(),
        body_prefix.trim().to_lowercase()
    );
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn build_preview_summary_from_fields(
    teaser: &str,
    subtitle: &str,
    body_text: &str,
) -> String {
    for candidate in [teaser, subtitle, body_text] {
        let preview = normalize_preview_text(candidate, 220);
        if !preview.is_empty() {
            return preview;
        }
    }

    String::new()
}

pub(super) fn normalize_preview_text(value: &str, max_chars: usize) -> String {
    let collapsed = value
        .trim()
        .strip_prefix("## ")
        .unwrap_or(value.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return String::new();
    }

    if collapsed.chars().count() <= max_chars {
        collapsed.to_owned()
    } else {
        format!(
            "{}...",
            collapsed
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        )
    }
}

pub(super) fn build_fts_query(search: Option<&str>) -> Option<String> {
    let search = search?.trim();
    if search.is_empty() {
        return None;
    }

    let tokens = search
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| format!("{token}*"))
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}
