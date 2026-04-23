use crate::app_paths;
use anyhow::{Context, Result, bail};
use regex::Regex;
use reqwest::{StatusCode, blocking::Client};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    hash::{Hash, Hasher},
    sync::mpsc,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

const BASE_URL: &str = "https://www.soziopolis.de";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BROWSE_SECTION_WORKERS: usize = 4;
const MAX_SECTION_PAGE_DEPTH: usize = 80;
const HTML_CACHE_TTL: Duration = Duration::from_secs(180);
const HTML_DISK_CACHE_TTL: Duration = Duration::from_secs(900);
const HTML_CACHE_CAPACITY: usize = 96;

static HTML_CACHE: OnceLock<Mutex<HashMap<String, CachedHtml>>> = OnceLock::new();

#[derive(Clone)]
struct CachedHtml {
    fetched_at: Instant,
    body: String,
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
    pub exhausted: bool,
}

#[derive(Debug, Clone)]
pub struct SectionBrowseState {
    pub articles: Vec<ArticleSummary>,
    pub report: DiscoveryReport,
    pub exhausted: bool,
    section: Section,
    pending_page_urls: VecDeque<String>,
    discovered_page_urls: HashSet<String>,
    seen_article_urls: HashSet<String>,
    visited_page_urls: HashSet<String>,
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

#[derive(Clone)]
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
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_TIMEOUT)
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

    pub fn start_section_browse(&self, section: &Section) -> Result<SectionBrowseState> {
        let first_page_url = section.url.to_owned();
        let first_page_html = self.fetch_html(&first_page_url)?;
        let first_page_document = Html::parse_document(&first_page_html);
        let mut articles = Vec::new();
        let mut seen_article_urls = HashSet::new();
        let mut report = DiscoveryReport::default();

        report.record_source_visit(DiscoverySourceKind::Section);
        self.collect_articles_from_document(
            &first_page_document,
            Some(section.label),
            &first_page_url,
            DiscoverySourceKind::Section,
            usize::MAX,
            &mut seen_article_urls,
            &mut articles,
            &mut report,
        );

        let pending_page_urls =
            section_page_urls(section, &first_page_html, MAX_SECTION_PAGE_DEPTH)
                .into_iter()
                .collect::<VecDeque<_>>();
        let discovered_page_urls = pending_page_urls.iter().cloned().collect::<HashSet<_>>();
        let mut visited_page_urls = HashSet::new();
        visited_page_urls.insert(first_page_url);

        Ok(SectionBrowseState {
            articles,
            report,
            exhausted: pending_page_urls.is_empty(),
            section: *section,
            pending_page_urls,
            discovered_page_urls,
            seen_article_urls,
            visited_page_urls,
        })
    }

    pub fn grow_section_browse(
        &self,
        state: &mut SectionBrowseState,
        target_limit: usize,
    ) -> Result<()> {
        let target_limit = target_limit.max(state.articles.len());
        let max_pages = desired_section_page_count(target_limit);

        while state.articles.len() < target_limit {
            let Some(page_url) = state.pending_page_urls.pop_front() else {
                state.exhausted = true;
                break;
            };

            if state.visited_page_urls.len() >= max_pages {
                state.exhausted = state.pending_page_urls.is_empty();
                break;
            }
            if !state.visited_page_urls.insert(page_url.clone()) {
                continue;
            }

            let html = self.fetch_html(&page_url)?;
            let document = Html::parse_document(&html);
            state
                .report
                .record_source_visit(DiscoverySourceKind::Section);
            self.collect_articles_from_document(
                &document,
                Some(state.section.label),
                &page_url,
                DiscoverySourceKind::Section,
                target_limit,
                &mut state.seen_article_urls,
                &mut state.articles,
                &mut state.report,
            );

            for discovered_url in
                extract_paginated_section_urls(&state.section, &html, MAX_SECTION_PAGE_DEPTH)
            {
                if state.discovered_page_urls.insert(discovered_url.clone()) {
                    state.pending_page_urls.push_back(discovered_url);
                }
            }
        }

        if state.pending_page_urls.is_empty() {
            state.exhausted = true;
        }

        Ok(())
    }

    pub fn browse_section_detailed(
        &self,
        section: &Section,
        limit: usize,
    ) -> Result<BrowseSectionResult> {
        let mut state = self.start_section_browse(section)?;
        self.grow_section_browse(&mut state, limit)?;

        Ok(BrowseSectionResult {
            articles: state.articles,
            report: state.report,
            exhausted: state.exhausted,
        })
    }

    pub fn browse_all_sections_detailed(&self, total_limit: usize) -> Result<BrowseSectionResult> {
        let mut state = self.start_all_sections_browse()?;
        self.grow_all_sections_browse(&mut state, total_limit)
    }

    pub fn start_all_sections_browse(&self) -> Result<AllSectionsBrowseState> {
        let worker_count = browse_section_worker_count(self.sections().len().max(1));
        let mut ordered_states = Vec::new();

        for chunk in self.sections().chunks(worker_count) {
            let (tx, rx) = mpsc::channel();
            thread::scope(|scope| {
                for (offset, section) in chunk.iter().enumerate() {
                    let tx = tx.clone();
                    let scraper = self.clone();
                    let section = *section;
                    scope.spawn(move || {
                        let result = scraper.start_section_browse(&section);
                        let _ = tx.send((offset, result));
                    });
                }
            });
            drop(tx);

            let mut chunk_results = Vec::new();
            while let Ok((offset, result)) = rx.recv() {
                chunk_results.push((offset, result?));
            }
            chunk_results.sort_by_key(|(offset, _)| *offset);
            ordered_states.extend(chunk_results.into_iter().map(|(_, state)| state));
        }

        Ok(AllSectionsBrowseState {
            section_states: ordered_states,
        })
    }

    pub fn grow_all_sections_browse(
        &self,
        state: &mut AllSectionsBrowseState,
        total_limit: usize,
    ) -> Result<BrowseSectionResult> {
        let section_count = state.section_states.len().max(1);
        let per_section_limit = total_limit.div_ceil(section_count).clamp(8, 20);
        let worker_count = browse_section_worker_count(section_count);

        for chunk in state
            .section_states
            .iter()
            .cloned()
            .enumerate()
            .collect::<Vec<_>>()
            .chunks(worker_count)
        {
            let (tx, rx) = mpsc::channel();
            thread::scope(|scope| {
                for (index, section_state) in chunk.iter().cloned() {
                    let tx = tx.clone();
                    let scraper = self.clone();
                    scope.spawn(move || {
                        let mut section_state = section_state;
                        let result = scraper
                            .grow_section_browse(&mut section_state, per_section_limit)
                            .map(|_| section_state);
                        let _ = tx.send((index, result));
                    });
                }
            });
            drop(tx);

            while let Ok((index, result)) = rx.recv() {
                state.section_states[index] = result?;
            }
        }

        Ok(merge_all_sections_states(
            &state.section_states,
            total_limit,
        ))
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

        let published_at = normalize_article_date(&date).unwrap_or_default();

        Ok(Article {
            url: url.to_owned(),
            title,
            subtitle,
            teaser: String::new(),
            author,
            date,
            published_at,
            section,
            source_kind: "article".to_owned(),
            source_label: source_label(url),
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
        if let Some(cached) = lookup_cached_html(url) {
            return Ok(cached);
        }

        let mut last_error = None;

        for attempt in 1..=3 {
            match self.client.get(url).send() {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        let body = response
                            .text()
                            .with_context(|| format!("network: failed to read body for {url}"));
                        if let Ok(body) = body {
                            store_cached_html(url, &body);
                            store_disk_cached_html(url, &body);
                            return Ok(body);
                        }
                        return body;
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

fn section_page_urls(section: &Section, first_page_html: &str, limit: usize) -> Vec<String> {
    let desired_pages = desired_section_page_count(limit);
    let mut discovered = extract_paginated_section_urls(section, first_page_html, desired_pages);
    if discovered.is_empty() {
        discovered = legacy_section_page_urls(section, desired_pages);
    }
    discovered
}

fn lookup_cached_html(url: &str) -> Option<String> {
    if let Some(body) = lookup_memory_cached_html(url) {
        return Some(body);
    }

    let body = lookup_disk_cached_html(url)?;
    store_memory_cached_html(url, &body);
    Some(body)
}

fn lookup_memory_cached_html(url: &str) -> Option<String> {
    let mut cache = html_cache().lock().expect("html cache mutex poisoned");
    let cached = cache.get(url)?;
    if cached.fetched_at.elapsed() <= HTML_CACHE_TTL {
        return Some(cached.body.clone());
    }

    cache.remove(url);
    None
}

fn store_cached_html(url: &str, body: &str) {
    store_memory_cached_html(url, body);
}

fn store_memory_cached_html(url: &str, body: &str) {
    let mut cache = html_cache().lock().expect("html cache mutex poisoned");
    if cache.len() >= HTML_CACHE_CAPACITY && !cache.contains_key(url) {
        let stalest_key = cache
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at)
            .map(|(key, _)| key.clone());
        if let Some(stalest_key) = stalest_key {
            cache.remove(&stalest_key);
        }
    }
    cache.insert(
        url.to_owned(),
        CachedHtml {
            fetched_at: Instant::now(),
            body: body.to_owned(),
        },
    );
}

fn lookup_disk_cached_html(url: &str) -> Option<String> {
    let path = browse_cache_path(url).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = modified.elapsed().ok()?;
    if age > HTML_DISK_CACHE_TTL {
        let _ = fs::remove_file(&path);
        return None;
    }

    fs::read_to_string(path).ok()
}

fn store_disk_cached_html(url: &str, body: &str) {
    let Ok(path) = browse_cache_path(url) else {
        return;
    };
    if let Err(err) = fs::write(&path, body) {
        crate::logging::warn(format!(
            "could not write browse cache file {}: {err}",
            path.display()
        ));
    }
}

fn browse_cache_path(url: &str) -> Result<std::path::PathBuf> {
    Ok(app_paths::browse_cache_dir()?.join(format!("{}.html", hash_url(url))))
}

fn hash_url(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn desired_section_page_count(limit: usize) -> usize {
    limit.max(20).div_ceil(10).clamp(1, MAX_SECTION_PAGE_DEPTH)
}

fn extract_paginated_section_urls(
    section: &Section,
    html: &str,
    desired_pages: usize,
) -> Vec<String> {
    let selector = match Selector::parse("a[href]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    let document = Html::parse_document(html);
    let section_path = section.url.trim_start_matches(BASE_URL);
    let page_re = match Regex::new(r"page%5D=(\d+)") {
        Ok(regex) => regex,
        Err(_) => return Vec::new(),
    };
    let mut seen = HashSet::new();
    let mut paginated = Vec::new();

    for link in document.select(&selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let href = href.replace("&amp;", "&");
        if !href.contains("page%5D=") || !href.contains("controller%5D=Search") {
            continue;
        }
        if !href.starts_with(section_path) && !href.starts_with(section.url) {
            continue;
        }

        let Some(page_number) = page_re
            .captures(&href)
            .and_then(|captures| captures.get(1))
            .and_then(|capture| capture.as_str().parse::<usize>().ok())
        else {
            continue;
        };

        if page_number <= 1 {
            continue;
        }

        let normalized_url = if href.starts_with("http") {
            href
        } else {
            format!("{BASE_URL}{href}")
        };

        if seen.insert(page_number) {
            paginated.push((page_number, normalized_url));
        }
    }

    paginated.sort_by_key(|(page_number, _)| *page_number);
    paginated
        .into_iter()
        .take(desired_pages.saturating_sub(1))
        .map(|(_, url)| url)
        .collect()
}

fn legacy_section_page_urls(section: &Section, desired_pages: usize) -> Vec<String> {
    let mut urls = Vec::new();
    for page in 2..=desired_pages {
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

fn html_cache() -> &'static Mutex<HashMap<String, CachedHtml>> {
    HTML_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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

pub fn normalize_article_date(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    for format in ["%d.%m.%Y", "%Y-%m-%d"] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(trimmed, format) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }

    trimmed
        .get(..10)
        .and_then(|prefix| chrono::NaiveDate::parse_from_str(prefix, "%Y-%m-%d").ok())
        .map(|date| date.format("%Y-%m-%d").to_string())
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

fn browse_section_worker_count(section_count: usize) -> usize {
    section_count.clamp(1, MAX_BROWSE_SECTION_WORKERS)
}

fn merge_all_sections_states(
    section_states: &[SectionBrowseState],
    total_limit: usize,
) -> BrowseSectionResult {
    let mut merged_articles = Vec::new();
    let mut merged_report = DiscoveryReport::default();
    let mut seen = HashSet::new();

    for section_state in section_states {
        merged_report.merge(&section_state.report);
        for article in &section_state.articles {
            if merged_articles.len() >= total_limit {
                break;
            }
            if seen.insert(article.url.clone()) {
                merged_articles.push(article.clone());
            } else {
                merged_report.deduped_articles += 1;
            }
        }
        if merged_articles.len() >= total_limit {
            break;
        }
    }

    BrowseSectionResult {
        articles: merged_articles,
        report: merged_report,
        exhausted: section_states.iter().all(|state| state.exhausted),
    }
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

    #[test]
    fn real_essay_listing_fixture_preserves_author_date_and_teaser() -> Result<()> {
        let client = SoziopolisClient::new()?;
        let document = Html::parse_document(include_str!(
            "../tests/fixtures/soziopolis_real_essay_listing_fixture.html"
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

        assert!(articles.len() >= 2);
        assert_eq!(articles[0].title, "Buchempfehlungen zum Frühling");
        assert_eq!(articles[0].date, "10.04.2026");
        assert!(articles[0].author.contains("Stephanie Kappacher"));
        assert!(articles[0].teaser.contains("Lektüretipps"));
        Ok(())
    }

    #[test]
    fn real_interview_listing_fixture_preserves_article_metadata() -> Result<()> {
        let client = SoziopolisClient::new()?;
        let document = Html::parse_document(include_str!(
            "../tests/fixtures/soziopolis_real_interview_listing_fixture.html"
        ));

        let mut seen = HashSet::new();
        let mut articles = Vec::new();
        let mut report = DiscoveryReport::default();

        client.collect_articles_from_document(
            &document,
            Some("Interviews"),
            "https://www.soziopolis.de/texte/interview.html",
            DiscoverySourceKind::Section,
            10,
            &mut seen,
            &mut articles,
            &mut report,
        );

        assert!(articles.len() >= 2);
        assert_eq!(
            articles[0].title,
            "„Tierrechte sind juridische Bauchrednerei“"
        );
        assert_eq!(articles[0].date, "04.03.2026");
        assert_eq!(articles[0].section, "Interview");
        assert!(articles[0].author.contains("Gonzalo Haefner"));
        Ok(())
    }

    #[test]
    fn section_page_urls_expand_with_higher_limits() {
        let html = r#"
            <html><body>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=1&amp;cHash=aaa">1</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=2&amp;cHash=bbb">2</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=3&amp;cHash=ccc">3</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=4&amp;cHash=ddd">4</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=5&amp;cHash=eee">5</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=6&amp;cHash=fff">6</a>
                <a href="/texte/interview.html?listArticles13%5Bcontroller%5D=Search&amp;listArticles13%5Bpage%5D=7&amp;cHash=ggg">7</a>
            </body></html>
        "#;
        let urls = section_page_urls(&SECTIONS[3], html, 80);
        assert_eq!(urls.len(), 6);
        assert!(urls[0].contains("listArticles13%5Bpage%5D=2"));
        assert!(urls[0].contains("cHash="));
    }

    #[test]
    fn deeper_interview_fixture_discovers_later_pages() {
        let html = include_str!("../tests/fixtures/soziopolis_real_interview_page10_fixture.html");
        let urls = extract_paginated_section_urls(&SECTIONS[3], html, 20);
        assert!(urls.iter().any(|url| url.contains("page%5D=11")));
        assert!(urls.iter().any(|url| url.contains("page%5D=12")));
        assert!(urls.iter().any(|url| url.contains("page%5D=13")));
    }

    #[test]
    fn merge_all_sections_states_marks_exhausted_only_when_every_section_is_done() {
        let state_a = SectionBrowseState {
            articles: vec![ArticleSummary {
                url: "https://example.com/a".to_owned(),
                title: "A".to_owned(),
                teaser: String::new(),
                author: String::new(),
                date: String::new(),
                section: "Essay".to_owned(),
                source_kind: DiscoverySourceKind::Section,
                source_label: "Essays".to_owned(),
            }],
            report: DiscoveryReport::default(),
            exhausted: true,
            section: SECTIONS[1],
            pending_page_urls: VecDeque::new(),
            discovered_page_urls: HashSet::new(),
            seen_article_urls: HashSet::new(),
            visited_page_urls: HashSet::new(),
        };
        let state_b = SectionBrowseState {
            articles: vec![ArticleSummary {
                url: "https://example.com/b".to_owned(),
                title: "B".to_owned(),
                teaser: String::new(),
                author: String::new(),
                date: String::new(),
                section: "Interview".to_owned(),
                source_kind: DiscoverySourceKind::Section,
                source_label: "Interviews".to_owned(),
            }],
            report: DiscoveryReport::default(),
            exhausted: false,
            section: SECTIONS[3],
            pending_page_urls: VecDeque::from([String::from("https://example.com/page2")]),
            discovered_page_urls: HashSet::new(),
            seen_article_urls: HashSet::new(),
            visited_page_urls: HashSet::new(),
        };

        let merged = merge_all_sections_states(&[state_a.clone(), state_b.clone()], 20);
        assert_eq!(merged.articles.len(), 2);
        assert!(!merged.exhausted);

        let merged_exhausted = merge_all_sections_states(
            &[
                state_a,
                SectionBrowseState {
                    exhausted: true,
                    ..state_b
                },
            ],
            20,
        );
        assert!(merged_exhausted.exhausted);
    }

    #[test]
    fn browse_section_worker_count_is_bounded() {
        assert_eq!(browse_section_worker_count(0), 1);
        assert_eq!(browse_section_worker_count(1), 1);
        assert_eq!(browse_section_worker_count(3), 3);
        assert_eq!(browse_section_worker_count(20), 4);
    }

    #[test]
    fn desired_section_page_count_scales_but_stays_bounded() {
        assert_eq!(desired_section_page_count(0), 2);
        assert_eq!(desired_section_page_count(80), 8);
        assert_eq!(desired_section_page_count(160), 16);
        assert_eq!(desired_section_page_count(5000), 80);
    }
}
