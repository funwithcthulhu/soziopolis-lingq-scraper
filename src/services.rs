use crate::{
    context::AppContext,
    database::StoredArticle,
    domain::ArticleListItem,
    jobs::{FailedFetchItem, ImportProgress, UploadFailure, UploadProgress, UploadSuccess},
    lingq::{Collection, LingqClient, UploadRequest},
    repositories::ArticleRepository,
    soziopolis::{
        AllSectionsBrowseState, Article, ArticleSummary, BrowseSectionResult, SectionBrowseState,
        SoziopolisClient, normalize_article_date,
    },
};
use anyhow::{Result, anyhow};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

const MAX_IMPORT_WORKERS: usize = 4;
const DEFAULT_MAX_UPLOAD_WORKERS: usize = 2;
const TESTED_MAX_UPLOAD_WORKERS: usize = 3;

pub struct ContentRefreshResult {
    pub imported_urls: Result<HashSet<String>, String>,
    pub library_articles: Result<Vec<ArticleListItem>, String>,
    pub library_stats: Result<crate::database::LibraryStats, String>,
}

pub struct ImportOutcome {
    pub saved_count: usize,
    pub saved_articles: Vec<ArticleListItem>,
    pub skipped_existing: usize,
    pub skipped_out_of_range: usize,
    pub failed: Vec<FailedFetchItem>,
    pub canceled: bool,
}

pub struct UploadOutcome {
    pub uploaded: usize,
    pub successes: Vec<UploadSuccess>,
    pub failed: Vec<UploadFailure>,
    pub canceled: bool,
}

pub struct BrowseResponse {
    pub articles: Vec<ArticleSummary>,
    pub report: crate::soziopolis::DiscoveryReport,
    pub exhausted: bool,
    pub session_state: Option<BrowseSessionState>,
}

#[derive(Clone)]
pub enum BrowseSessionState {
    CurrentSection(SectionBrowseState),
    AllSections(AllSectionsBrowseState),
}

pub struct BrowseService;

impl BrowseService {
    pub fn browse_section(section_id: &str, limit: usize) -> Result<BrowseResponse> {
        let started = Instant::now();
        let scraper = SoziopolisClient::new()?;
        let section = scraper
            .section_by_id(section_id)
            .ok_or_else(|| anyhow!("unknown section '{section_id}'"))?;
        let mut state = scraper.start_section_browse(section)?;
        scraper.grow_section_browse(&mut state, limit)?;
        crate::logging::info(format!(
            "browse_section '{}' loaded {} article(s) in {:?}",
            section_id,
            state.articles.len(),
            started.elapsed()
        ));
        Ok(BrowseResponse {
            articles: state.articles.clone(),
            report: state.report.clone(),
            exhausted: state.exhausted,
            session_state: Some(BrowseSessionState::CurrentSection(state)),
        })
    }

    pub fn continue_browse_section(
        mut state: SectionBrowseState,
        limit: usize,
    ) -> Result<BrowseResponse> {
        let started = Instant::now();
        let scraper = SoziopolisClient::new()?;
        scraper.grow_section_browse(&mut state, limit)?;
        crate::logging::info(format!(
            "continue_browse_section now has {} article(s) in {:?}",
            state.articles.len(),
            started.elapsed()
        ));
        Ok(BrowseResponse {
            articles: state.articles.clone(),
            report: state.report.clone(),
            exhausted: state.exhausted,
            session_state: Some(BrowseSessionState::CurrentSection(state)),
        })
    }

    pub fn browse_all_sections(limit: usize) -> Result<BrowseResponse> {
        let started = Instant::now();
        let scraper = SoziopolisClient::new()?;
        let mut state = scraper.start_all_sections_browse()?;
        let result: BrowseSectionResult = scraper.grow_all_sections_browse(&mut state, limit)?;
        crate::logging::info(format!(
            "browse_all_sections loaded {} article(s) in {:?}",
            result.articles.len(),
            started.elapsed()
        ));
        Ok(BrowseResponse {
            articles: result.articles,
            report: result.report,
            exhausted: result.exhausted,
            session_state: Some(BrowseSessionState::AllSections(state)),
        })
    }

    pub fn continue_browse_all_sections(
        mut state: AllSectionsBrowseState,
        limit: usize,
    ) -> Result<BrowseResponse> {
        let started = Instant::now();
        let scraper = SoziopolisClient::new()?;
        let result = scraper.grow_all_sections_browse(&mut state, limit)?;
        crate::logging::info(format!(
            "continue_browse_all_sections now has {} article(s) in {:?}",
            result.articles.len(),
            started.elapsed()
        ));
        Ok(BrowseResponse {
            articles: result.articles,
            report: result.report,
            exhausted: result.exhausted,
            session_state: Some(BrowseSessionState::AllSections(state)),
        })
    }

    pub fn preview_article(url: &str) -> Result<Article> {
        SoziopolisClient::new()?.fetch_article(url)
    }

    pub fn import_articles(
        app_context: &AppContext,
        articles: Vec<ArticleSummary>,
        cancel_flag: Arc<AtomicBool>,
        mut on_progress: impl FnMut(ImportProgress),
    ) -> Result<ImportOutcome> {
        let started = Instant::now();
        let mut saved_count = 0usize;
        let mut saved_articles = Vec::new();
        let mut skipped_existing = 0usize;
        let mut failed = Vec::new();
        let total = articles.len();
        let mut canceled = false;
        let shared_db = app_context.db.clone();
        let mut known_urls = shared_db.with_db(|db| {
            let repository = ArticleRepository::new(db);
            repository.get_all_article_urls()
        })?;
        let mut processed = 0usize;
        let mut pending = Vec::new();
        let mut fetched_articles = Vec::new();

        for summary in articles {
            let current_item = if summary.title.is_empty() {
                summary.url.clone()
            } else {
                summary.title.clone()
            };

            if known_urls.contains(&summary.url) {
                skipped_existing += 1;
                processed += 1;
                on_progress(ImportProgress {
                    phase: "Scanning selected articles".to_owned(),
                    processed,
                    total: Some(total),
                    saved_count,
                    skipped_existing,
                    skipped_out_of_range: 0,
                    failed_count: failed.len(),
                    current_item,
                });
                continue;
            }

            pending.push(summary);
        }

        let worker_count = import_worker_count(pending.len());
        let queue = Arc::new(Mutex::new(VecDeque::from(pending)));
        let (result_tx, result_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = Arc::clone(&queue);
                let result_tx = result_tx.clone();
                let cancel_flag = Arc::clone(&cancel_flag);
                scope.spawn(move || {
                    worker_fetch_articles(queue, result_tx, cancel_flag);
                });
            }
            drop(result_tx);

            while let Ok(result) = result_rx.recv() {
                processed += 1;
                match result.outcome {
                    Ok(article) => {
                        let fingerprint = crate::database::debug_article_fingerprint(&article);
                        let duplicate = shared_db
                            .with_db(|db| {
                                let repository = ArticleRepository::new(db);
                                Ok(repository
                                    .get_article_id_by_fingerprint(&fingerprint)?
                                    .is_some())
                            })
                            .unwrap_or(false);
                        if duplicate {
                            skipped_existing += 1;
                        } else {
                            fetched_articles.push(article);
                        }
                    }
                    Err(message) => failed.push(FailedFetchItem {
                        url: result.summary.url.clone(),
                        title: result.summary.title.clone(),
                        category: "fetch".to_owned(),
                        message,
                    }),
                }

                on_progress(ImportProgress {
                    phase: "Fetching selected articles".to_owned(),
                    processed,
                    total: Some(total),
                    saved_count,
                    skipped_existing,
                    skipped_out_of_range: 0,
                    failed_count: failed.len(),
                    current_item: result.current_item,
                });
            }
        });

        if cancel_flag.load(Ordering::Relaxed) {
            canceled = true;
        }

        if !fetched_articles.is_empty() {
            let save_total = fetched_articles.len();
            on_progress(ImportProgress {
                phase: "Saving imported articles".to_owned(),
                processed: 0,
                total: Some(save_total),
                saved_count,
                skipped_existing,
                skipped_out_of_range: 0,
                failed_count: failed.len(),
                current_item: "Writing articles to the local library".to_owned(),
            });

            match shared_db.with_db(|db| db.save_articles_batch(&fetched_articles)) {
                Ok(mut stored_articles) => {
                    saved_count += stored_articles.len();
                    for article in &stored_articles {
                        known_urls.insert(article.url.clone());
                    }
                    saved_articles.extend(stored_articles.drain(..).map(ArticleListItem::from));
                }
                Err(batch_err) => {
                    crate::logging::warn(format!(
                        "batch import save failed; retrying individual article writes: {batch_err}"
                    ));
                    for article in fetched_articles {
                        match shared_db.with_db(|db| db.save_article(&article)) {
                            Ok(_) => match shared_db.with_db(|db| db.get_article_id_by_url(&article.url)) {
                                Ok(Some(id)) => match shared_db.with_db(|db| db.get_article(id)) {
                                    Ok(Some(stored)) => {
                                        saved_count += 1;
                                        known_urls.insert(stored.url.clone());
                                        saved_articles.push(ArticleListItem::from(stored));
                                    }
                                    Ok(None) => failed.push(FailedFetchItem {
                                        url: article.url.clone(),
                                        title: article.title.clone(),
                                        category: "database".to_owned(),
                                        message:
                                            "Article saved but could not be reloaded from the local library."
                                                .to_owned(),
                                    }),
                                    Err(err) => failed.push(FailedFetchItem {
                                        url: article.url.clone(),
                                        title: article.title.clone(),
                                        category: "database".to_owned(),
                                        message: err.to_string(),
                                    }),
                                },
                                Ok(None) => failed.push(FailedFetchItem {
                                    url: article.url.clone(),
                                    title: article.title.clone(),
                                    category: "database".to_owned(),
                                    message:
                                        "Article saved but no local article id was returned."
                                            .to_owned(),
                                }),
                                Err(err) => failed.push(FailedFetchItem {
                                    url: article.url.clone(),
                                    title: article.title.clone(),
                                    category: "database".to_owned(),
                                    message: err.to_string(),
                                }),
                            },
                            Err(err) => failed.push(FailedFetchItem {
                                url: article.url.clone(),
                                title: article.title.clone(),
                                category: "database".to_owned(),
                                message: err.to_string(),
                            }),
                        }
                    }
                }
            }

            on_progress(ImportProgress {
                phase: "Saving imported articles".to_owned(),
                processed: save_total,
                total: Some(save_total),
                saved_count,
                skipped_existing,
                skipped_out_of_range: 0,
                failed_count: failed.len(),
                current_item: "Local library updated".to_owned(),
            });
        }

        Ok(ImportOutcome {
            saved_count,
            saved_articles,
            skipped_existing,
            skipped_out_of_range: 0,
            failed,
            canceled,
        })
        .inspect(|outcome| {
            crate::logging::info(format!(
                "import_articles processed {} input item(s): saved {}, skipped {}, failed {} in {:?}",
                total,
                outcome.saved_count,
                outcome.skipped_existing,
                outcome.failed.len(),
                started.elapsed()
            ));
        })
    }
}

struct ImportFetchResult {
    summary: ArticleSummary,
    current_item: String,
    outcome: Result<Article, String>,
}

struct UploadArticleResult {
    article: crate::database::StoredArticle,
    current_item: String,
    outcome: Result<crate::lingq::UploadResponse, String>,
}

fn worker_fetch_articles(
    queue: Arc<Mutex<VecDeque<ArticleSummary>>>,
    result_tx: mpsc::Sender<ImportFetchResult>,
    cancel_flag: Arc<AtomicBool>,
) {
    let scraper = match SoziopolisClient::new() {
        Ok(scraper) => scraper,
        Err(err) => {
            let message = err.to_string();
            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                let next_summary = {
                    let mut queue = queue.lock().expect("import queue mutex poisoned");
                    queue.pop_front()
                };
                let Some(summary) = next_summary else {
                    break;
                };
                let current_item = if summary.title.is_empty() {
                    summary.url.clone()
                } else {
                    summary.title.clone()
                };
                let _ = result_tx.send(ImportFetchResult {
                    summary,
                    current_item,
                    outcome: Err(message.clone()),
                });
            }
            return;
        }
    };

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let next_summary = {
            let mut queue = queue.lock().expect("import queue mutex poisoned");
            queue.pop_front()
        };
        let Some(summary) = next_summary else {
            break;
        };
        let current_item = if summary.title.is_empty() {
            summary.url.clone()
        } else {
            summary.title.clone()
        };
        let outcome = fetch_article_for_import(&scraper, &summary).map_err(|err| err.to_string());
        let _ = result_tx.send(ImportFetchResult {
            summary,
            current_item,
            outcome,
        });
    }
}

fn fetch_article_for_import(
    scraper: &SoziopolisClient,
    summary: &ArticleSummary,
) -> Result<Article> {
    let mut article = scraper.fetch_article(&summary.url)?;
    article.teaser = summary.teaser.clone();
    article.author = if article.author.trim().is_empty() {
        summary.author.clone()
    } else {
        article.author.clone()
    };
    article.date = if article.date.trim().is_empty() {
        summary.date.clone()
    } else {
        article.date.clone()
    };
    article.published_at = normalize_article_date(&article.date)
        .or_else(|| normalize_article_date(&summary.date))
        .unwrap_or_default();
    article.section = if article.section.trim().is_empty() {
        summary.section.clone()
    } else {
        article.section.clone()
    };
    article.source_kind = summary.source_kind.as_str().to_owned();
    article.source_label = summary.source_label.clone();
    Ok(article)
}

fn import_worker_count(pending_items: usize) -> usize {
    pending_items.clamp(1, MAX_IMPORT_WORKERS)
}

fn upload_worker_count(pending_items: usize) -> usize {
    pending_items.clamp(1, configured_upload_worker_cap())
}

fn configured_upload_worker_cap() -> usize {
    configured_upload_worker_cap_from_env(
        std::env::var("SOZIOPOLIS_LINGQ_UPLOAD_WORKERS")
            .ok()
            .as_deref(),
    )
}

fn configured_upload_worker_cap_from_env(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_WORKERS)
        .clamp(1, TESTED_MAX_UPLOAD_WORKERS)
}

fn worker_upload_articles(
    queue: Arc<Mutex<VecDeque<crate::database::StoredArticle>>>,
    result_tx: mpsc::Sender<UploadArticleResult>,
    cancel_flag: Arc<AtomicBool>,
    api_key: String,
    collection_id: Option<i64>,
    worker_index: usize,
) {
    let lingq = match LingqClient::new() {
        Ok(client) => client,
        Err(err) => {
            let message = err.to_string();
            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                let next_article = {
                    let mut queue = queue.lock().expect("upload queue mutex poisoned");
                    queue.pop_front()
                };
                let Some(article) = next_article else {
                    break;
                };
                let _ = result_tx.send(UploadArticleResult {
                    current_item: article.title.clone(),
                    article,
                    outcome: Err(message.clone()),
                });
            }
            return;
        }
    };

    if worker_index > 0 {
        std::thread::sleep(std::time::Duration::from_millis(175 * worker_index as u64));
    }

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let next_article = {
            let mut queue = queue.lock().expect("upload queue mutex poisoned");
            queue.pop_front()
        };
        let Some(article) = next_article else {
            break;
        };

        let current_item = article.title.clone();
        let outcome = lingq
            .upload_lesson(&UploadRequest {
                api_key: api_key.clone(),
                language_code: "de".to_owned(),
                collection_id,
                title: article.title.clone(),
                text: article.clean_text.clone(),
                original_url: Some(article.url.clone()),
            })
            .map_err(|err| err.to_string());

        let _ = result_tx.send(UploadArticleResult {
            article,
            current_item,
            outcome,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{configured_upload_worker_cap_from_env, import_worker_count, upload_worker_count};

    #[test]
    fn import_worker_count_is_bounded() {
        assert_eq!(import_worker_count(0), 1);
        assert_eq!(import_worker_count(1), 1);
        assert_eq!(import_worker_count(3), 3);
        assert_eq!(import_worker_count(20), 4);
    }

    #[test]
    fn upload_worker_count_is_bounded() {
        assert_eq!(upload_worker_count(0), 1);
        assert_eq!(upload_worker_count(1), 1);
        assert_eq!(upload_worker_count(2), 2);
        assert_eq!(upload_worker_count(12), 2);
    }

    #[test]
    fn configured_upload_worker_cap_defaults_to_two() {
        assert_eq!(configured_upload_worker_cap_from_env(None), 2);
        assert_eq!(configured_upload_worker_cap_from_env(Some("3")), 3);
        assert_eq!(configured_upload_worker_cap_from_env(Some("5")), 3);
    }
}

pub struct LibraryService;

impl LibraryService {
    pub fn refresh_content(app_context: &AppContext) -> ContentRefreshResult {
        let started = Instant::now();
        let result = app_context
            .db
            .with_db(|db| {
                let repository = ArticleRepository::new(db);
                Ok(ContentRefreshResult {
                    imported_urls: repository
                        .get_all_article_urls()
                        .map_err(|err| err.to_string()),
                    library_articles: repository
                        .list_article_cards(None, None, false)
                        .map_err(|err| err.to_string()),
                    library_stats: repository.get_stats().map_err(|err| err.to_string()),
                })
            })
            .unwrap_or_else(|err| {
                let message = err.to_string();
                ContentRefreshResult {
                    imported_urls: Err(message.clone()),
                    library_articles: Err(message.clone()),
                    library_stats: Err(message),
                }
            });
        crate::logging::info(format!(
            "refresh_content completed in {:?}",
            started.elapsed()
        ));
        result
    }

    pub fn delete_article(app_context: &AppContext, id: i64) -> Result<()> {
        app_context.db.with_db(|db| {
            let repository = ArticleRepository::new(db);
            repository.delete_article(id)
        })
    }

    pub fn get_article(app_context: &AppContext, id: i64) -> Result<Option<StoredArticle>> {
        app_context.db.with_db(|db| {
            let repository = ArticleRepository::new(db);
            repository.get_article(id)
        })
    }
}

pub struct LingqService;

impl LingqService {
    pub fn login(username: &str, password: &str) -> Result<String> {
        Ok(LingqClient::new()?.login(username, password)?.token)
    }

    pub fn collections(api_key: &str, language_code: &str) -> Result<Vec<Collection>> {
        LingqClient::new()?.get_collections(api_key, language_code)
    }

    pub fn upload_articles(
        app_context: &AppContext,
        ids: Vec<i64>,
        api_key: String,
        collection_id: Option<i64>,
        cancel_flag: Arc<AtomicBool>,
        mut on_progress: impl FnMut(UploadProgress),
    ) -> Result<UploadOutcome> {
        let started = Instant::now();
        let shared_db = app_context.db.clone();
        let mut uploaded = 0usize;
        let mut successes = Vec::new();
        let mut failed = Vec::new();
        let total = ids.len();
        let mut canceled = false;
        let ordered_ids = ids;
        let article_map = shared_db
            .with_db(|db| {
                let repository = ArticleRepository::new(db);
                repository.get_articles_by_ids(&ordered_ids)
            })?
            .into_iter()
            .map(|article| (article.id, article))
            .collect::<HashMap<_, _>>();
        let mut queue = VecDeque::new();

        for id in ordered_ids {
            if let Some(article) = article_map.get(&id) {
                queue.push_back(article.clone());
            } else {
                failed.push(UploadFailure {
                    article_id: id,
                    title: format!("Article #{id}"),
                    message: "Article not found in the local library.".to_owned(),
                });
            }
        }

        let initial_processed = failed.len();
        if initial_processed > 0 {
            on_progress(UploadProgress {
                processed: initial_processed,
                total,
                uploaded,
                failed_count: failed.len(),
                current_item: "Preparing LingQ upload queue".to_owned(),
            });
        }

        let worker_count = upload_worker_count(queue.len());
        crate::logging::info(format!(
            "LingQ upload worker cap resolved to {} (pending items: {})",
            worker_count,
            queue.len()
        ));
        let queue = Arc::new(Mutex::new(queue));
        let (result_tx, result_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            for worker_index in 0..worker_count {
                let queue = Arc::clone(&queue);
                let result_tx = result_tx.clone();
                let cancel_flag = Arc::clone(&cancel_flag);
                let api_key = api_key.clone();
                scope.spawn(move || {
                    worker_upload_articles(
                        queue,
                        result_tx,
                        cancel_flag,
                        api_key,
                        collection_id,
                        worker_index,
                    );
                });
            }
            drop(result_tx);

            let mut processed = initial_processed;
            while let Ok(result) = result_rx.recv() {
                processed += 1;
                match result.outcome {
                    Ok(response) => {
                        match shared_db.with_db(|db| {
                            let repository = ArticleRepository::new(db);
                            repository.mark_uploaded(
                                result.article.id,
                                response.lesson_id,
                                &response.lesson_url,
                            )
                        }) {
                            Ok(()) => {
                                uploaded += 1;
                                successes.push(UploadSuccess {
                                    article_id: result.article.id,
                                    lesson_id: response.lesson_id,
                                    lesson_url: response.lesson_url,
                                });
                            }
                            Err(err) => failed.push(UploadFailure {
                                article_id: result.article.id,
                                title: result.article.title.clone(),
                                message: err.to_string(),
                            }),
                        }
                    }
                    Err(message) => failed.push(UploadFailure {
                        article_id: result.article.id,
                        title: result.article.title.clone(),
                        message,
                    }),
                }

                on_progress(UploadProgress {
                    processed,
                    total,
                    uploaded,
                    failed_count: failed.len(),
                    current_item: result.current_item,
                });
            }
        });

        if cancel_flag.load(Ordering::Relaxed) {
            canceled = true;
        }

        Ok(UploadOutcome {
            uploaded,
            successes,
            failed,
            canceled,
        })
        .inspect(|outcome| {
            crate::logging::info(format!(
                "upload_articles processed {} item(s): uploaded {}, failed {} in {:?}",
                total,
                outcome.uploaded,
                outcome.failed.len(),
                started.elapsed()
            ));
        })
    }
}
