#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySortMode {
    Newest,
    Oldest,
    Longest,
    Shortest,
    Title,
}

impl LibrarySortMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::Longest => "Longest",
            Self::Shortest => "Shortest",
            Self::Title => "Title",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArticleListItem {
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
    pub word_count: i64,
    pub fetched_at: String,
    pub custom_topic: String,
    pub uploaded_to_lingq: bool,
    pub lingq_lesson_id: Option<i64>,
    pub lingq_lesson_url: String,
}

#[derive(Debug, Clone)]
pub struct ArticleListPage {
    pub items: Vec<ArticleListItem>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
}

impl From<crate::database::StoredArticle> for ArticleListItem {
    fn from(value: crate::database::StoredArticle) -> Self {
        Self {
            id: value.id,
            url: value.url,
            title: value.title,
            subtitle: value.subtitle,
            teaser: value.teaser,
            preview_summary: value.preview_summary,
            author: value.author,
            date: value.date,
            published_at: value.published_at,
            section: value.section,
            word_count: value.word_count,
            fetched_at: value.fetched_at,
            custom_topic: value.custom_topic,
            uploaded_to_lingq: value.uploaded_to_lingq,
            lingq_lesson_id: value.lingq_lesson_id,
            lingq_lesson_url: value.lingq_lesson_url,
        }
    }
}
