use crate::{
    database::{Database, LibraryStats, StoredArticle},
    domain::ArticleListItem,
    jobs::{CompletedJob, QueueSnapshot},
    soziopolis::Article,
};
use anyhow::Result;
use std::collections::HashSet;

pub struct ArticleRepository<'a> {
    db: &'a Database,
}

impl<'a> ArticleRepository<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn save_article(&self, article: &Article) -> Result<i64> {
        self.db.save_article(article)
    }

    pub fn list_articles(
        &self,
        search: Option<&str>,
        section: Option<&str>,
        only_not_uploaded: bool,
        limit: usize,
    ) -> Result<Vec<StoredArticle>> {
        self.db
            .list_articles(search, section, only_not_uploaded, limit)
    }

    pub fn list_article_cards(
        &self,
        search: Option<&str>,
        section: Option<&str>,
        only_not_uploaded: bool,
    ) -> Result<Vec<ArticleListItem>> {
        self.db
            .list_article_cards(search, section, only_not_uploaded)
    }

    pub fn get_article(&self, id: i64) -> Result<Option<StoredArticle>> {
        self.db.get_article(id)
    }

    pub fn get_articles_by_ids(&self, ids: &[i64]) -> Result<Vec<StoredArticle>> {
        self.db.get_articles_by_ids(ids)
    }

    pub fn get_articles_by_urls(&self, urls: &[&str]) -> Result<Vec<StoredArticle>> {
        self.db.get_articles_by_urls(urls)
    }

    pub fn get_article_id_by_url(&self, url: &str) -> Result<Option<i64>> {
        self.db.get_article_id_by_url(url)
    }

    pub fn get_article_id_by_fingerprint(&self, fingerprint: &str) -> Result<Option<i64>> {
        self.db.get_article_id_by_fingerprint(fingerprint)
    }

    pub fn get_all_article_urls(&self) -> Result<HashSet<String>> {
        self.db.get_all_article_urls()
    }

    pub fn mark_uploaded(&self, id: i64, lesson_id: i64, lesson_url: &str) -> Result<()> {
        self.db.mark_uploaded(id, lesson_id, lesson_url)
    }

    pub fn delete_article(&self, id: i64) -> Result<()> {
        self.db.delete_article(id)
    }

    pub fn get_stats(&self) -> Result<LibraryStats> {
        self.db.get_stats()
    }
}

pub struct JobRepository<'a> {
    db: &'a mut Database,
}

impl<'a> JobRepository<'a> {
    pub fn new(db: &'a mut Database) -> Self {
        Self { db }
    }

    pub fn load_snapshot(&self) -> Result<QueueSnapshot> {
        self.db.load_queue_snapshot()
    }

    pub fn save_snapshot(&mut self, snapshot: &QueueSnapshot) -> Result<()> {
        self.db.save_queue_snapshot(snapshot)
    }

    pub fn record_completed_job_history(&self, job: &CompletedJob) -> Result<()> {
        self.db.record_completed_job_history(job)
    }

    pub fn list_completed_job_history(&self, limit: usize) -> Result<Vec<CompletedJob>> {
        self.db.list_completed_job_history(limit)
    }
}
