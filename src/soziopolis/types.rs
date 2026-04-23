use super::*;

#[derive(Clone)]
pub(super) struct CachedHtml {
    pub(super) fetched_at: Instant,
    pub(super) body: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleSummary {
    pub url: String,
    pub title: String,
    pub teaser: String,
    pub author: String,
    pub date: String,
    pub section: String,
    pub source_kind: DiscoverySourceKind,
    pub source_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoverySourceKind {
    Section,
    Subsection,
    Topic,
}

impl DiscoverySourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Subsection => "subsection",
            Self::Topic => "topic",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryReport {
    pub source_pages_visited: usize,
    pub section_pages_visited: usize,
    pub subsection_pages_visited: usize,
    pub topic_pages_visited: usize,
    pub section_articles: usize,
    pub subsection_articles: usize,
    pub topic_articles: usize,
    pub deduped_articles: usize,
}

impl DiscoveryReport {
    pub(super) fn record_source_visit(&mut self, source_kind: DiscoverySourceKind) {
        self.source_pages_visited += 1;
        match source_kind {
            DiscoverySourceKind::Section => self.section_pages_visited += 1,
            DiscoverySourceKind::Subsection => self.subsection_pages_visited += 1,
            DiscoverySourceKind::Topic => self.topic_pages_visited += 1,
        }
    }

    pub(super) fn record_article(&mut self, source_kind: DiscoverySourceKind) {
        match source_kind {
            DiscoverySourceKind::Section => self.section_articles += 1,
            DiscoverySourceKind::Subsection => self.subsection_articles += 1,
            DiscoverySourceKind::Topic => self.topic_articles += 1,
        }
    }

    pub(super) fn merge(&mut self, other: &Self) {
        self.source_pages_visited += other.source_pages_visited;
        self.section_pages_visited += other.section_pages_visited;
        self.subsection_pages_visited += other.subsection_pages_visited;
        self.topic_pages_visited += other.topic_pages_visited;
        self.section_articles += other.section_articles;
        self.subsection_articles += other.subsection_articles;
        self.topic_articles += other.topic_articles;
        self.deduped_articles += other.deduped_articles;
    }
}

#[derive(Debug, Clone)]
pub struct BrowseSectionResult {
    pub articles: Vec<ArticleSummary>,
    pub report: DiscoveryReport,
    pub exhausted: bool,
}

#[derive(Debug, Clone)]
pub struct SectionBrowseState {
    pub articles: Vec<ArticleSummary>,
    pub report: DiscoveryReport,
    pub exhausted: bool,
    pub(super) section: Section,
    pub(super) pending_page_urls: VecDeque<String>,
    pub(super) discovered_page_urls: HashSet<String>,
    pub(super) seen_article_urls: HashSet<String>,
    pub(super) visited_page_urls: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct AllSectionsBrowseState {
    pub section_states: Vec<SectionBrowseState>,
}

#[derive(Debug, Clone)]
pub struct Article {
    pub url: String,
    pub title: String,
    pub subtitle: String,
    pub teaser: String,
    pub author: String,
    pub date: String,
    pub published_at: String,
    pub section: String,
    pub source_kind: String,
    pub source_label: String,
    pub body_text: String,
    pub clean_text: String,
    pub word_count: usize,
    pub fetched_at: String,
}

#[derive(Debug, Clone)]
pub struct ArticleMetadata {
    pub url: String,
    pub title: String,
    pub date: String,
    pub section: String,
}

pub const SECTIONS: &[Section] = &[
    Section {
        id: "latest",
        label: "Latest",
        url: "https://www.soziopolis.de/index.html",
    },
    Section {
        id: "essays",
        label: "Essays",
        url: "https://www.soziopolis.de/texte/essay.html",
    },
    Section {
        id: "reviews",
        label: "Besprechungen",
        url: "https://www.soziopolis.de/besprechungen.html",
    },
    Section {
        id: "interviews",
        label: "Interviews",
        url: "https://www.soziopolis.de/texte/interview.html",
    },
    Section {
        id: "dossiers",
        label: "Dossiers",
        url: "https://www.soziopolis.de/dossier.html",
    },
    Section {
        id: "soziales-leben",
        label: "Soziales Leben",
        url: "https://www.soziopolis.de/soziales-leben.html",
    },
    Section {
        id: "gesellschaftstheorie",
        label: "Gesellschaftstheorie & Anthropologie",
        url: "https://www.soziopolis.de/gesellschaftstheorie-anthropologie.html",
    },
    Section {
        id: "politik",
        label: "Politik & Zeitgeschichte",
        url: "https://www.soziopolis.de/politik-zeitgeschichte.html",
    },
    Section {
        id: "wirtschaft",
        label: "Wirtschaft & Recht",
        url: "https://www.soziopolis.de/wirtschaft-recht.html",
    },
    Section {
        id: "kultur",
        label: "Kultur & Medien",
        url: "https://www.soziopolis.de/kultur-medien.html",
    },
    Section {
        id: "wissenschaft",
        label: "Wissenschaft & Technik",
        url: "https://www.soziopolis.de/wissenschaft-technik.html",
    },
    Section {
        id: "zeitschriftenschau",
        label: "Zeitschriftenschau",
        url: "https://www.soziopolis.de/zeitschriftenschau.html",
    },
];

#[derive(Debug, Default)]
pub(super) struct ListingMetadata {
    pub(super) author: String,
    pub(super) date: String,
    pub(super) section: String,
}
