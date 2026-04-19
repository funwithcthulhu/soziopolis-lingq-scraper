use anyhow::{Context, Result, bail};
use regex::Regex;
use reqwest::{StatusCode, blocking::Client};
use scraper::{ElementRef, Html, Selector};
use std::{collections::HashSet, time::Duration};

const BASE_URL: &str = "https://www.soziopolis.de";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn record_source_visit(&mut self, source_kind: DiscoverySourceKind) {
        self.source_pages_visited += 1;
        match source_kind {
            DiscoverySourceKind::Section => self.section_pages_visited += 1,
            DiscoverySourceKind::Subsection => self.subsection_pages_visited += 1,
            DiscoverySourceKind::Topic => self.topic_pages_visited += 1,
        }
    }

    fn record_article(&mut self, source_kind: DiscoverySourceKind) {
        match source_kind {
            DiscoverySourceKind::Section => self.section_articles += 1,
            DiscoverySourceKind::Subsection => self.subsection_articles += 1,
            DiscoverySourceKind::Topic => self.topic_articles += 1,
        }
    }

    fn merge(&mut self, other: &Self) {
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
}

#[derive(Debug, Clone)]
pub struct Article {
    pub url: String,
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub date: String,
    pub section: String,
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

pub struct SoziopolisClient {
    client: Client,
    article_url_re: Regex,
}

#[derive(Debug, Default)]
struct ListingMetadata {
    author: String,
    date: String,
    section: String,
}

impl SoziopolisClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .context("failed to build HTTP client")?;
        let article_url_re =
            Regex::new(r"^https://www\.soziopolis\.de/.+\.html(?:\?.*)?$").context("bad regex")?;

        Ok(Self {
            client,
            article_url_re,
        })
    }

    pub fn sections(&self) -> &'static [Section] {
        SECTIONS
    }

    pub fn section_by_id(&self, id: &str) -> Option<&'static Section> {
        SECTIONS.iter().find(|section| section.id == id)
    }

    pub fn browse_section(&self, section: &Section, limit: usize) -> Result<Vec<ArticleSummary>> {
        Ok(self.browse_section_detailed(section, limit)?.articles)
    }

    pub fn browse_section_detailed(
        &self,
        section: &Section,
        limit: usize,
    ) -> Result<BrowseSectionResult> {
        let mut articles = Vec::new();
        let mut seen = HashSet::new();
        let mut report = DiscoveryReport::default();
        let page_urls = section_page_urls(section, limit);

        for page_url in page_urls {
            if articles.len() >= limit {
                break;
            }

            let html = self.fetch_html(&page_url)?;
            let document = Html::parse_document(&html);
            report.record_source_visit(DiscoverySourceKind::Section);
            self.collect_articles_from_document(
                &document,
                Some(section.label),
                &page_url,
                DiscoverySourceKind::Section,
                limit,
                &mut seen,
                &mut articles,
                &mut report,
            );
        }

        Ok(BrowseSectionResult { articles, report })
    }

    pub fn browse_all_sections_detailed(&self, total_limit: usize) -> Result<BrowseSectionResult> {
        let mut merged_articles = Vec::new();
        let mut merged_report = DiscoveryReport::default();
        let mut seen = HashSet::new();
        let section_count = self.sections().len().max(1);
        let per_section_limit = total_limit.div_ceil(section_count).clamp(8, 20);

        for section in self.sections() {
            if merged_articles.len() >= total_limit {
                break;
            }

            let section_result = self.browse_section_detailed(section, per_section_limit)?;
            merged_report.merge(&section_result.report);

            for article in section_result.articles {
                if merged_articles.len() >= total_limit {
                    break;
                }

                if seen.insert(article.url.clone()) {
                    merged_articles.push(article);
                } else {
                    merged_report.deduped_articles += 1;
                }
            }
        }

        Ok(BrowseSectionResult {
            articles: merged_articles,
            report: merged_report,
        })
    }

    pub fn browse_url(
        &self,
        url: &str,
        fallback_section: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ArticleSummary>> {
        let html = self.fetch_html(url)?;
        let document = Html::parse_document(&html);
        let mut articles = Vec::new();
        let mut seen = HashSet::new();
        let mut report = DiscoveryReport::default();
        self.collect_articles_from_document(
            &document,
            fallback_section,
            url,
            DiscoverySourceKind::Section,
            limit,
            &mut seen,
            &mut articles,
            &mut report,
        );
        Ok(articles)
    }

    pub fn fetch_article(&self, url: &str) -> Result<Article> {
        let html = self.fetch_html(url)?;
        let document = Html::parse_document(&html);

        let title = first_text(
            &document,
            &[
                "h1.article-title",
                "h1",
                "meta[property=\"og:title\"]",
                "title",
            ],
        )
        .map(|value| value.replace(" | Soziopolis", "").trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Untitled".to_owned());

        let subtitle = first_text(
            &document,
            &["h2.article-subtitle", "meta[name=\"description\"]"],
        )
        .unwrap_or_default();

        let author = collect_authors(&document);
        let date = first_text(
            &document,
            &[
                ".article-date",
                "time",
                "meta[property=\"article:published_time\"]",
            ],
        )
        .or_else(|| extract_date_from_html(&html))
        .unwrap_or_default();
        let section = extract_section(&document)
            .or_else(|| infer_section_from_url(url))
            .unwrap_or_else(|| "Soziopolis".to_owned());

        let body_text = extract_body(&document)?;
        let word_count = body_text.split_whitespace().count();
        if word_count < 80 {
            bail!("article extraction produced too little text for {url}");
        }

        let clean_text = build_clean_text(&title, &subtitle, &author, &date, &body_text);

        Ok(Article {
            url: url.to_owned(),
            title,
            subtitle,
            author,
            date,
            section,
            body_text,
            clean_text,
            word_count,
            fetched_at: iso_timestamp_now(),
        })
    }

    pub fn fetch_article_metadata(&self, url: &str) -> Result<ArticleMetadata> {
        let article = self.fetch_article(url)?;
        Ok(ArticleMetadata {
            url: article.url,
            title: article.title,
            date: article.date,
            section: article.section,
        })
    }

    fn fetch_html(&self, url: &str) -> Result<String> {
        let mut last_error = None;

        for attempt in 1..=3 {
            match self.client.get(url).send() {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return response
                            .text()
                            .with_context(|| format!("network: failed to read body for {url}"));
                    }

                    let retryable = is_retryable_status(status);
                    last_error = Some(anyhow::anyhow!(
                        "network: non-success response {} for {}",
                        status,
                        url
                    ));
                    if !retryable || attempt == 3 {
                        break;
                    }
                }
                Err(err) => {
                    last_error = Some(anyhow::anyhow!("network: request failed for {url}: {err}"));
                    if attempt == 3 {
                        break;
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(450 * attempt as u64));
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("network: failed to fetch {url}")))
    }

    fn collect_articles_from_document(
        &self,
        document: &Html,
        fallback_section: Option<&str>,
        source_url: &str,
        source_kind: DiscoverySourceKind,
        limit: usize,
        seen: &mut HashSet<String>,
        articles: &mut Vec<ArticleSummary>,
        report: &mut DiscoveryReport,
    ) {
        let headline_selector = Selector::parse("h2 a[href], h3 a[href]").expect("selector");
        for link in document.select(&headline_selector) {
            if articles.len() >= limit {
                break;
            }

            let Some(raw_href) = link.value().attr("href") else {
                continue;
            };
            let article_url = absolute_url(raw_href);
            if !self.article_url_re.is_match(&article_url) || is_excluded_article_url(&article_url)
            {
                continue;
            }
            if !seen.insert(article_url.clone()) {
                report.deduped_articles += 1;
                continue;
            }

            let title = clean_whitespace(&collect_text(link));
            if !looks_like_article_title(&title) {
                continue;
            }

            let teaser = extract_teaser_from_heading(link);
            let metadata = extract_listing_metadata(link, fallback_section, source_url);

            articles.push(ArticleSummary {
                url: article_url,
                title,
                teaser,
                author: metadata.author,
                date: metadata.date,
                section: metadata.section,
                source_kind,
                source_label: source_label(source_url),
            });
            report.record_article(source_kind);
        }
    }
}

fn section_page_urls(section: &Section, limit: usize) -> Vec<String> {
    let page_count = limit.max(20).div_ceil(10).clamp(1, 6);
    let mut urls = vec![section.url.to_owned()];
    for page in 1..page_count {
        if section.url.contains('?') {
            urls.push(format!(
                "{}&listArticles12%5Bcontroller%5D=Search&listArticles12%5Bpage%5D={page}",
                section.url
            ));
        } else {
            urls.push(format!(
                "{}?listArticles12%5Bcontroller%5D=Search&listArticles12%5Bpage%5D={page}",
                section.url
            ));
        }
    }
    urls
}

fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error()
        || matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT
        )
}

fn extract_body(document: &Html) -> Result<String> {
    let article_selector = Selector::parse(".article-content").expect("selector");
    let block_selector = Selector::parse("p, h2, h3, li, blockquote").expect("selector");
    let mut best_blocks = Vec::new();

    for article in document.select(&article_selector) {
        let mut blocks = Vec::new();
        for node in article.select(&block_selector) {
            let name = node.value().name();
            let mut text = clean_whitespace(&collect_text(node));
            text = normalize_inline_markers(&text);
            if should_skip_block(&text) {
                continue;
            }
            match name {
                "h2" | "h3" if text.len() >= 4 => blocks.push(format!("## {text}")),
                "li" if text.len() >= 16 => blocks.push(format!("- {text}")),
                "blockquote" if text.len() >= 30 => blocks.push(text),
                "p" if text.len() >= 35 => blocks.push(text),
                _ => {}
            }
        }
        if blocks.len() > best_blocks.len() {
            best_blocks = blocks;
        }
    }

    dedupe_lines(&mut best_blocks);
    if best_blocks.is_empty() {
        bail!("could not extract article body");
    }
    Ok(best_blocks.join("\n\n"))
}

fn should_skip_block(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let markers = [
        "Empfehlungen",
        "Artikel lesen",
        "Zur PDF-Datei dieses Artikels",
        "Social Science Open Access Repository",
        "ISSN 2509-5196",
        "Zum Seitenanfang",
    ];
    markers.iter().any(|marker| text.contains(marker))
}

fn normalize_inline_markers(text: &str) -> String {
    let footnote_re = Regex::new(r"\[\d+\]").expect("footnote regex");
    clean_whitespace(&footnote_re.replace_all(text, " "))
}

fn dedupe_lines(lines: &mut Vec<String>) {
    let mut seen = HashSet::new();
    lines.retain(|line| seen.insert(canonical_text(line)));
}

fn collect_authors(document: &Html) -> String {
    let primary_selector = Selector::parse("p.article-overline .author-name").expect("selector");
    let primary = document
        .select(&primary_selector)
        .map(collect_text)
        .map(|value| clean_whitespace(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if !primary.is_empty() {
        return primary.join(", ");
    }

    let fallback_selector =
        Selector::parse(".article-overline .author-name, .article-header .author-name")
            .expect("selector");
    let mut authors = Vec::new();
    let mut seen = HashSet::new();
    for node in document.select(&fallback_selector) {
        let value = clean_whitespace(&collect_text(node));
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        authors.push(value);
    }
    authors.join(", ")
}

fn extract_section(document: &Html) -> Option<String> {
    if let Some(article_type) = first_text(document, &[".article-type"]) {
        let normalized = clean_whitespace(&article_type);
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }

    let category_selector = Selector::parse("p.article-categories a").ok()?;
    for node in document.select(&category_selector) {
        let value = clean_whitespace(&collect_text(node));
        if !value.is_empty() {
            return Some(value);
        }
    }

    let keywords_selector = Selector::parse("meta[name=\"keywords\"]").ok()?;
    for node in document.select(&keywords_selector) {
        let value = node.value().attr("content").map(clean_whitespace)?;
        let first_keyword = value
            .split(',')
            .map(str::trim)
            .find(|part| !part.is_empty())
            .map(str::to_owned);
        if first_keyword.is_some() {
            return first_keyword;
        }
    }

    None
}

fn extract_date_from_html(html: &str) -> Option<String> {
    let re = Regex::new(r#"article-date\">\s*([^<]+)\s*<"#).ok()?;
    re.captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| clean_whitespace(value.as_str()))
}

fn extract_teaser_from_heading(link: ElementRef<'_>) -> String {
    let preferred_selectors = [
        "p.article-abstract",
        ".article-abstract",
        "p.article-subtitle",
        ".article-subtitle",
        ".list-text",
    ];
    let fallback_selector = Selector::parse("p").expect("selector");
    let link_text = collect_text(link);
    let mut parent = link.parent();
    for _ in 0..5 {
        let Some(node) = parent else {
            break;
        };
        if let Some(element) = ElementRef::wrap(node) {
            for selector in preferred_selectors {
                let selector = Selector::parse(selector).expect("selector");
                for candidate in element.select(&selector) {
                    let text = clean_whitespace(&collect_text(candidate));
                    if is_good_teaser_candidate(&text, &link_text) {
                        return trim_chars(&text, 240);
                    }
                }
            }

            for candidate in element.select(&fallback_selector) {
                let text = clean_whitespace(&collect_text(candidate));
                if is_good_teaser_candidate(&text, &link_text) {
                    return trim_chars(&text, 240);
                }
            }
        }
        parent = node.parent();
    }
    String::new()
}

fn extract_listing_metadata(
    link: ElementRef<'_>,
    fallback_section: Option<&str>,
    source_url: &str,
) -> ListingMetadata {
    let container = nearest_listing_container(link);
    let mut metadata = ListingMetadata::default();

    if let Some(container) = container {
        let author_selector = Selector::parse(".author-name").expect("selector");
        let date_selector = Selector::parse(".article-date, time").expect("selector");
        let section_selector =
            Selector::parse(".article-type, .article-categories a").expect("selector");

        let mut authors = Vec::new();
        let mut seen_authors = HashSet::new();
        for candidate in container.select(&author_selector) {
            let value = clean_whitespace(&collect_text(candidate));
            if !value.is_empty() && seen_authors.insert(value.clone()) {
                authors.push(value);
            }
        }
        metadata.author = authors.join(", ");

        metadata.date = container
            .select(&date_selector)
            .map(collect_text)
            .map(|value| clean_whitespace(&value))
            .find(|value| !value.is_empty())
            .unwrap_or_default();

        metadata.section = container
            .select(&section_selector)
            .map(collect_text)
            .map(|value| clean_whitespace(&value))
            .find(|value| !value.is_empty() && !value.contains('|'))
            .unwrap_or_default();
    }

    if metadata.section.is_empty() {
        metadata.section = fallback_section
            .map(str::to_owned)
            .or_else(|| extract_context_section(link))
            .unwrap_or_else(|| source_label(source_url));
    }

    metadata
}

fn is_good_teaser_candidate(text: &str, link_text: &str) -> bool {
    if text.len() < 18
        || same_enough(text, link_text)
        || text.contains("Artikel lesen")
        || text.contains('|')
    {
        return false;
    }

    let lower = text.to_lowercase();
    !lower.starts_with("von ")
        && !lower.contains("rezension |")
        && !lower.contains("interview |")
        && !lower.contains("essay |")
}

fn extract_context_section(link: ElementRef<'_>) -> Option<String> {
    let mut parent = link.parent();
    let selector =
        Selector::parse(".article-type, .article-overline, .article-categories a").ok()?;
    for _ in 0..4 {
        let Some(node) = parent else {
            break;
        };
        if let Some(element) = ElementRef::wrap(node) {
            for candidate in element.select(&selector) {
                let value = clean_whitespace(&collect_text(candidate));
                if !value.is_empty() && !value.contains('|') {
                    return Some(value);
                }
            }
        }
        parent = node.parent();
    }
    None
}

fn nearest_listing_container(link: ElementRef<'_>) -> Option<ElementRef<'_>> {
    let marker_selector = Selector::parse(
        ".article-overline, .article-abstract, .article-type, .article-date, .list-text",
    )
    .ok()?;

    link.ancestors()
        .filter_map(ElementRef::wrap)
        .take(6)
        .find(|element| element.select(&marker_selector).next().is_some())
}

fn source_label(source_url: &str) -> String {
    SECTIONS
        .iter()
        .find(|section| section.url.trim_end_matches('/') == source_url.trim_end_matches('/'))
        .map(|section| section.label.to_owned())
        .unwrap_or_else(|| {
            infer_section_from_url(source_url).unwrap_or_else(|| "Soziopolis".to_owned())
        })
}

fn infer_section_from_url(url: &str) -> Option<String> {
    let path = url.trim_start_matches(BASE_URL).trim_start_matches('/');
    let first = path.split('/').next()?;
    if first.is_empty() {
        return Some("Latest".to_owned());
    }
    Some(first.replace('-', " "))
}

fn build_clean_text(title: &str, subtitle: &str, author: &str, date: &str, body: &str) -> String {
    let normalized_subtitle = clean_whitespace(subtitle);
    let normalized_body = normalize_body_for_lingq(body, title, &normalized_subtitle);

    let mut pieces = vec![title.to_owned()];
    if !normalized_subtitle.is_empty() && !same_enough(&normalized_subtitle, title) {
        pieces.push(String::new());
        pieces.push(normalized_subtitle);
    }
    if !author.is_empty() {
        pieces.push(format!("Von {author}"));
    }
    if !date.is_empty() {
        pieces.push(date.to_owned());
    }
    pieces.push(String::new());
    pieces.push(normalized_body);
    pieces.join("\n")
}

fn normalize_body_for_lingq(body: &str, title: &str, subtitle: &str) -> String {
    let mut cleaned_blocks = Vec::new();
    for raw_block in body.split("\n\n") {
        let block = clean_whitespace(raw_block);
        if block.is_empty() {
            continue;
        }
        let normalized_block = if let Some(heading) = block.strip_prefix("## ") {
            heading.trim().to_owned()
        } else {
            block
        };
        if same_enough(&normalized_block, title)
            || (!subtitle.is_empty() && same_enough(&normalized_block, subtitle))
        {
            continue;
        }
        cleaned_blocks.push(normalized_block);
    }
    dedupe_similar_blocks(&mut cleaned_blocks);
    cleaned_blocks.join("\n\n")
}

fn dedupe_similar_blocks(blocks: &mut Vec<String>) {
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(canonical_text(block)));
}

fn canonical_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn same_enough(left: &str, right: &str) -> bool {
    let left = canonical_text(left);
    let right = canonical_text(right);
    !left.is_empty() && left == right
}

fn first_text(document: &Html, selectors: &[&str]) -> Option<String> {
    for selector in selectors {
        let selector = Selector::parse(selector).ok()?;
        let value = document.select(&selector).find_map(|node| {
            let attr_content = node.value().attr("content").map(clean_whitespace);
            let text_content =
                Some(clean_whitespace(&collect_text(node))).filter(|value| !value.is_empty());
            attr_content.or(text_content)
        });
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            return Some(value);
        }
    }
    None
}

fn absolute_url(raw_href: &str) -> String {
    if raw_href.starts_with("http://") || raw_href.starts_with("https://") {
        return raw_href.to_owned();
    }
    if raw_href.starts_with('/') {
        return format!("{BASE_URL}{raw_href}");
    }
    format!("{BASE_URL}/{raw_href}")
}

fn looks_like_article_title(title: &str) -> bool {
    title.len() >= 10
        && title.len() <= 220
        && !title.eq_ignore_ascii_case("Artikel lesen")
        && !title.eq_ignore_ascii_case("Essays")
        && !title.eq_ignore_ascii_case("Besprechungen")
        && !title.eq_ignore_ascii_case("Interviews")
        && !title.eq_ignore_ascii_case("Dossiers")
}

fn is_excluded_article_url(url: &str) -> bool {
    let path = url.trim_start_matches(BASE_URL).trim_start_matches('/');

    let exact = [
        "index.html",
        "suche.html",
        "newsletter.html",
        "veroeffentlichen.html",
        "kontakt.html",
        "partner.html",
        "ueber-uns.html",
        "rssfeed.xml",
        "texte/essay.html",
        "texte/interview.html",
        "texte/podcast-video.html",
        "besprechungen.html",
        "dossier.html",
        "soziales-leben.html",
        "gesellschaftstheorie-anthropologie.html",
        "politik-zeitgeschichte.html",
        "wirtschaft-recht.html",
        "kultur-medien.html",
        "wissenschaft-technik.html",
        "zeitschriftenschau.html",
    ];

    if exact.contains(&path) {
        return true;
    }

    [
        "autoren/",
        "ausschreibungen/",
        "buchforum/",
        "meta/",
        "fileadmin/",
        "dossier/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

fn collect_text(node: ElementRef<'_>) -> String {
    node.text().collect::<Vec<_>>().join(" ")
}

fn clean_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_chars(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

fn iso_timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_fixture_extracts_expected_fields() -> Result<()> {
        let html = include_str!("../tests/fixtures/soziopolis_article_fixture.html");
        let document = Html::parse_document(html);

        let title = first_text(
            &document,
            &[
                "h1.article-title",
                "h1",
                "meta[property=\"og:title\"]",
                "title",
            ],
        )
        .expect("fixture title");
        let subtitle = first_text(
            &document,
            &["h2.article-subtitle", "meta[name=\"description\"]"],
        )
        .expect("fixture subtitle");
        let author = collect_authors(&document);
        let section = extract_section(&document).expect("fixture section");
        let body = extract_body(&document)?;
        let clean_text = build_clean_text(&title, &subtitle, &author, "2026-02-19", &body);

        assert_eq!(title, "Im Strudel des Digitalen");
        assert_eq!(
            subtitle,
            "Rezension zu \"Der Stachel des Digitalen. Geisteswissenschaften und Digital Humanities\" von Sybille Kraemer"
        );
        assert_eq!(author, "Sybille Kraemer");
        assert_eq!(section, "Essay");
        assert!(body.contains("## Zwischen Daten und Deutung"));
        assert!(body.contains("Der Stachel des Digitalen"));
        assert!(!body.contains("Artikel lesen"));
        assert!(clean_text.contains("Von Sybille Kraemer"));
        Ok(())
    }

    #[test]
    fn section_fixture_discovers_unique_articles_and_teasers() -> Result<()> {
        let client = SoziopolisClient::new()?;
        let document = Html::parse_document(include_str!(
            "../tests/fixtures/soziopolis_section_fixture.html"
        ));

        let mut seen = HashSet::new();
        let mut articles = Vec::new();
        let mut report = DiscoveryReport::default();

        client.collect_articles_from_document(
            &document,
            Some("Essays"),
            "https://www.soziopolis.de/texte/essay.html",
            DiscoverySourceKind::Section,
            10,
            &mut seen,
            &mut articles,
            &mut report,
        );

        assert_eq!(articles.len(), 2);
        assert_eq!(report.section_articles, 2);
        assert_eq!(report.deduped_articles, 1);
        assert_eq!(articles[0].title, "Im Strudel des Digitalen");
        assert_eq!(articles[0].author, "Sybille Kraemer");
        assert_eq!(articles[0].date, "19.02.2026");
        assert!(articles[0].teaser.contains("Geisteswissenschaften"));
        assert_eq!(articles[1].title, "Mood Tracker");
        assert_eq!(articles[1].author, "Test Autorin");
        assert_eq!(articles[1].date, "17.02.2026");
        assert!(articles[1].teaser.contains("Selbstvermessung"));
        Ok(())
    }
}
