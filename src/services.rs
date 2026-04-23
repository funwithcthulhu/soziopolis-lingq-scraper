use crate::{
    database::Database,
    jobs::{FailedFetchItem, ImportProgress, UploadFailure, UploadProgress},
    lingq::{Collection, LingqClient, UploadRequest},
    repositories::ArticleRepository,
    soziopolis::{
        AllSectionsBrowseState, Article, ArticleSummary, BrowseSectionResult, SectionBrowseState,
        SoziopolisClient, normalize_article_date,
    },
};
use anyhow::{Result, anyhow};
use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_IMPORT_WORKERS: usize = 4;

pub struct ContentRefreshResult {
    pub imported_urls: Result<HashSet<String>, String>,
    pub library_articles: Result<Vec<crate::database::StoredArticle>, String>,
    pub library_stats: Result<crate::database::LibraryStats, String>,
}

pub struct ImportOutcome {
    pub saved_count: usize,
    pub skipped_existing: usize,
    pub skipped_out_of_range: usize,
    pub failed: Vec<FailedFetchItem>,
    pub canceled: bool,
}

pub struct UploadOutcome {
    pub uploaded: usize,
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
        let scraper = SoziopolisClient::new()?;
        let section = scraper
            .section_by_id(section_id)
            .ok_or_else(|| anyhow!("unknown section '{section_id}'"))?;
        let mut state = scraper.start_section_browse(section)?;
        scraper.grow_section_browse(&mut state, limit)?;
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
        let scraper = SoziopolisClient::new()?;
        scraper.grow_section_browse(&mut state, limit)?;
        Ok(BrowseResponse {
            articles: state.articles.clone(),
            report: state.report.clone(),
            exhausted: state.exhausted,
            session_state: Some(BrowseSessionState::CurrentSection(state)),
        })
    }

    pub fn browse_all_sections(limit: usize) -> Result<BrowseResponse> {
        let scraper = SoziopolisClient::new()?;
        let mut state = scraper.start_all_sections_browse()?;
        let result: BrowseSectionResult = scraper.grow_all_sections_browse(&mut state, limit)?;
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
        let scraper = SoziopolisClient::new()?;
        let result = scraper.grow_all_sections_browse(&mut state, limit)?;
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
        articles: Vec<ArticleSummary>,
        cancel_flag: Arc<AtomicBool>,
        mut on_progress: impl FnMut(ImportProgress),
    ) -> Result<ImportOutcome> {
        let db = Database::open_default()?;
        let repository = ArticleRepository::new(&db);
        let mut saved_count = 0usize;
        let mut skipped_existing = 0usize;
        let mut failed = Vec::new();
        let total = articles.len();
        let mut canceled = false;
        let mut known_urls = repository.get_all_article_urls()?;
        let mut processed = 0usize;
        let mut pending = Vec::new();

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
                    phase: "Importing selected articles".to_owned(),
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
                        if let Err(err) = repository.save_article(&article) {
                            failed.push(FailedFetchItem {
                                url: result.summary.url.clone(),
                                title: article.title.clone(),
                                category: "database".to_owned(),
                                message: err.to_string(),
                            });
                        } else {
                            saved_count += 1;
                            known_urls.insert(result.summary.url.clone());
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
                    phase: "Importing selected articles".to_owned(),
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

        Ok(ImportOutcome {
            saved_count,
            skipped_existing,
            skipped_out_of_range: 0,
            failed,
            canceled,
        })
    }
}

struct ImportFetchResult {
    summary: ArticleSummary,
    current_item: String,
    outcome: Result<Article, String>,
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

#[cfg(test)]
mod tests {
    use super::import_worker_count;

    #[test]
    fn import_worker_count_is_bounded() {
        assert_eq!(import_worker_count(0), 1);
        assert_eq!(import_worker_count(1), 1);
        assert_eq!(import_worker_count(3), 3);
        assert_eq!(import_worker_count(20), 4);
    }
}

pub struct LibraryService;

impl LibraryService {
    pub fn refresh_content() -> ContentRefreshResult {
        let db = match Database::open_default() {
            Ok(db) => db,
            Err(err) => {
                let message = err.to_string();
                return ContentRefreshResult {
                    imported_urls: Err(message.clone()),
                    library_articles: Err(message.clone()),
                    library_stats: Err(message),
                };
            }
        };
        let repository = ArticleRepository::new(&db);
        ContentRefreshResult {
            imported_urls: repository
                .get_all_article_urls()
                .map_err(|err| err.to_string()),
            library_articles: repository
                .list_articles(None, None, false, 0)
                .map_err(|err| err.to_string()),
            library_stats: repository.get_stats().map_err(|err| err.to_string()),
        }
    }

    pub fn delete_article(id: i64) -> Result<()> {
        let db = Database::open_default()?;
        let repository = ArticleRepository::new(&db);
        repository.delete_article(id)
    }

    pub fn get_article(id: i64) -> Result<Option<crate::database::StoredArticle>> {
        let db = Database::open_default()?;
        let repository = ArticleRepository::new(&db);
        repository.get_article(id)
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
        ids: Vec<i64>,
        api_key: String,
        collection_id: Option<i64>,
        cancel_flag: Arc<AtomicBool>,
        mut on_progress: impl FnMut(UploadProgress),
    ) -> Result<UploadOutcome> {
        let db = Database::open_default()?;
        let repository = ArticleRepository::new(&db);
        let lingq = LingqClient::new()?;
        let mut uploaded = 0usize;
        let mut failed = Vec::new();
        let total = ids.len();
        let mut canceled = false;

        for (index, id) in ids.into_iter().enumerate() {
            if cancel_flag.load(Ordering::Relaxed) {
                canceled = true;
                break;
            }

            let Some(article) = repository.get_article(id)? else {
                failed.push(UploadFailure {
                    article_id: id,
                    title: format!("Article #{id}"),
                    message: "Article not found in the local library.".to_owned(),
                });
                continue;
            };

            let current_item = article.title.clone();
            let upload = lingq.upload_lesson(&UploadRequest {
                api_key: api_key.clone(),
                language_code: "de".to_owned(),
                collection_id,
                title: article.title.clone(),
                text: article.clean_text.clone(),
                original_url: Some(article.url.clone()),
            });

            match upload {
                Ok(response) => {
                    repository.mark_uploaded(
                        article.id,
                        response.lesson_id,
                        &response.lesson_url,
                    )?;
                    uploaded += 1;
                }
                Err(err) => failed.push(UploadFailure {
                    article_id: article.id,
                    title: article.title.clone(),
                    message: err.to_string(),
                }),
            }

            on_progress(UploadProgress {
                processed: index + 1,
                total,
                uploaded,
                failed_count: failed.len(),
                current_item,
            });
        }

        Ok(UploadOutcome {
            uploaded,
            failed,
            canceled,
        })
    }
}
