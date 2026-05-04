use super::*;

pub(super) fn section_page_urls(
    section: &Section,
    first_page_html: &str,
    limit: usize,
) -> Vec<String> {
    let desired_pages = desired_section_page_count(limit);
    let mut discovered = extract_paginated_section_urls(section, first_page_html, desired_pages);
    if discovered.is_empty() {
        discovered = legacy_section_page_urls(section, desired_pages);
    }
    discovered
}

pub(super) fn lookup_cached_html(url: &str) -> Option<String> {
    if let Some(body) = lookup_memory_cached_html(url) {
        return Some(body);
    }

    let body = lookup_disk_cached_html(url)?;
    store_memory_cached_html(url, &body);
    Some(body)
}

pub(super) fn lookup_memory_cached_html(url: &str) -> Option<String> {
    let mut cache = html_cache().lock().expect("html cache mutex poisoned");
    let cached = cache.get(url)?;
    if cached.fetched_at.elapsed() <= HTML_CACHE_TTL {
        return Some(cached.body.clone());
    }

    cache.remove(url);
    None
}

pub(super) fn store_cached_html(url: &str, body: &str) {
    store_memory_cached_html(url, body);
}

pub(super) fn store_memory_cached_html(url: &str, body: &str) {
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

pub(super) fn lookup_disk_cached_html(url: &str) -> Option<String> {
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

pub(super) fn store_disk_cached_html(url: &str, body: &str) {
    let Ok(path) = browse_cache_path(url) else {
        return;
    };
    if let Err(err) = fs::write(&path, body) {
        crate::logging::warn(format!(
            "could not write browse cache file {}: {err}",
            path.display()
        ));
        return;
    }

    prune_disk_browse_cache();
}

pub(super) fn browse_cache_path(url: &str) -> Result<std::path::PathBuf> {
    Ok(app_paths::browse_cache_dir()?.join(format!("{}.html", hash_url(url))))
}

pub(super) fn summary_cache() -> &'static Mutex<HashMap<String, Vec<ArticleSummary>>> {
    SUMMARY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn article_summary_cache_key(
    source_url: &str,
    fallback_section: Option<&str>,
    source_kind: DiscoverySourceKind,
    document_fingerprint: &str,
) -> String {
    format!(
        "{}|{}|{}|{}",
        source_url,
        fallback_section.unwrap_or_default(),
        source_kind.as_str(),
        document_fingerprint
    )
}

pub(super) fn document_summary_fingerprint(document: &Html) -> String {
    let headline_selector = match Selector::parse("h2 a[href], h3 a[href]") {
        Ok(selector) => selector,
        Err(_) => return String::from("selector-error"),
    };
    let mut signature = String::new();
    let mut count = 0usize;
    for link in document.select(&headline_selector).take(12) {
        let href = link.value().attr("href").unwrap_or_default();
        let title = clean_whitespace(&collect_text(link));
        signature.push_str(href);
        signature.push('|');
        signature.push_str(&title);
        signature.push('\n');
        count += 1;
    }
    signature.push_str(&format!("count:{count}"));
    hash_url(&signature)
}

pub(super) fn cached_article_summaries_for_source(
    scraper: &SoziopolisClient,
    document: &Html,
    fallback_section: Option<&str>,
    source_url: &str,
    source_kind: DiscoverySourceKind,
) -> Vec<ArticleSummary> {
    let cache_key = article_summary_cache_key(
        source_url,
        fallback_section,
        source_kind,
        &document_summary_fingerprint(document),
    );
    if let Ok(cache) = summary_cache().lock()
        && let Some(summaries) = cache.get(&cache_key)
    {
        crate::perf::record_browse_summary_cache_hit();
        return summaries.clone();
    }
    crate::perf::record_browse_summary_cache_miss();

    let headline_selector = Selector::parse("h2 a[href], h3 a[href]").expect("selector");
    let mut summaries = Vec::new();
    for link in document.select(&headline_selector) {
        let Some(raw_href) = link.value().attr("href") else {
            continue;
        };
        let article_url = absolute_url(raw_href);
        if !scraper.article_url_re.is_match(&article_url) || is_excluded_article_url(&article_url) {
            continue;
        }

        let title = clean_whitespace(&collect_text(link));
        if !looks_like_article_title(&title) {
            continue;
        }

        let teaser = extract_teaser_from_heading(link);
        let metadata = extract_listing_metadata(link, fallback_section, source_url);
        summaries.push(ArticleSummary {
            url: article_url,
            title,
            teaser,
            author: metadata.author,
            date: metadata.date,
            section: metadata.section,
            source_kind,
            source_label: source_label(source_url),
        });
    }

    if let Ok(mut cache) = summary_cache().lock() {
        if cache.len() > HTML_CACHE_CAPACITY * 2 {
            cache.clear();
        }
        cache.insert(cache_key, summaries.clone());
    }
    summaries
}

pub(super) fn hash_url(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn prune_disk_browse_cache() {
    let Ok(cache_dir) = app_paths::browse_cache_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&cache_dir) else {
        return;
    };

    let mut files = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let modified = metadata.modified().ok()?;
            let age = modified.elapsed().ok()?;
            Some((entry.path(), modified, age))
        })
        .collect::<Vec<_>>();

    for (path, _, age) in &files {
        if *age > HTML_DISK_CACHE_TTL {
            let _ = fs::remove_file(path);
        }
    }

    files.retain(|(_, _, age)| *age <= HTML_DISK_CACHE_TTL);
    if files.len() <= HTML_DISK_CACHE_FILE_CAPACITY {
        return;
    }

    files.sort_by_key(|(_, modified, _)| *modified);
    let remove_count = files.len().saturating_sub(HTML_DISK_CACHE_FILE_CAPACITY);
    for (path, _, _) in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

pub(super) fn desired_section_page_count(limit: usize) -> usize {
    limit.max(20).div_ceil(10).clamp(1, MAX_SECTION_PAGE_DEPTH)
}

pub(super) fn extract_paginated_section_urls(
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

pub(super) fn legacy_section_page_urls(section: &Section, desired_pages: usize) -> Vec<String> {
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

pub(super) fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error()
        || matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT
        )
}

pub(super) fn html_cache() -> &'static Mutex<HashMap<String, CachedHtml>> {
    HTML_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clear_browse_cache() -> Result<usize> {
    let cache_dir = app_paths::browse_cache_dir()?;
    let mut removed = 0usize;
    if let Ok(mut cache) = html_cache().lock() {
        cache.clear();
    }
    if let Ok(mut cache) = summary_cache().lock() {
        cache.clear();
    }
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}
