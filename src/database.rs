use crate::{
    app_paths,
    jobs::{CompletedJob, QueueSnapshot},
    soziopolis::Article,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::{collections::HashSet, path::Path, time::Duration};

const CURRENT_SCHEMA_VERSION: i32 = 6;

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

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let db_path = app_paths::database_path()?;
        Self::open(&db_path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database {}", path.display()))?;
        let database = Self { conn };
        database.configure_connection()?;
        database.migrate()?;
        Ok(database)
    }

    pub fn save_article(&self, article: &Article) -> Result<i64> {
        let preview_summary = build_article_preview_summary(article);
        let mut save_stmt = self.conn.prepare_cached(
            r#"
            INSERT INTO articles (
                url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                source_label, body_text, clean_text, word_count, fetched_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                subtitle = excluded.subtitle,
                teaser = excluded.teaser,
                preview_summary = excluded.preview_summary,
                author = excluded.author,
                date = excluded.date,
                published_at = excluded.published_at,
                section = excluded.section,
                source_kind = excluded.source_kind,
                source_label = excluded.source_label,
                body_text = excluded.body_text,
                clean_text = excluded.clean_text,
                word_count = excluded.word_count,
                fetched_at = excluded.fetched_at
            "#,
        )?;
        save_stmt.execute(params![
            article.url,
            article.title,
            article.subtitle,
            article.teaser,
            preview_summary,
            article.author,
            article.date,
            article.published_at,
            article.section,
            article.source_kind,
            article.source_label,
            article.body_text,
            article.clean_text,
            article.word_count as i64,
            article.fetched_at,
        ])?;

        let mut id_stmt = self
            .conn
            .prepare_cached("SELECT id FROM articles WHERE url = ?1")?;
        let id: i64 = id_stmt.query_row(params![article.url], |row| row.get(0))?;

        Ok(id)
    }

    pub fn save_articles_batch(&mut self, articles: &[Article]) -> Result<Vec<StoredArticle>> {
        if articles.is_empty() {
            return Ok(Vec::new());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut save_stmt = transaction.prepare(
                r#"
                INSERT INTO articles (
                    url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                    source_label, body_text, clean_text, word_count, fetched_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                ON CONFLICT(url) DO UPDATE SET
                    title = excluded.title,
                    subtitle = excluded.subtitle,
                    teaser = excluded.teaser,
                    preview_summary = excluded.preview_summary,
                    author = excluded.author,
                    date = excluded.date,
                    published_at = excluded.published_at,
                    section = excluded.section,
                    source_kind = excluded.source_kind,
                    source_label = excluded.source_label,
                    body_text = excluded.body_text,
                    clean_text = excluded.clean_text,
                    word_count = excluded.word_count,
                    fetched_at = excluded.fetched_at
                "#,
            )?;

            for article in articles {
                save_stmt.execute(params![
                    article.url,
                    article.title,
                    article.subtitle,
                    article.teaser,
                    build_article_preview_summary(article),
                    article.author,
                    article.date,
                    article.published_at,
                    article.section,
                    article.source_kind,
                    article.source_label,
                    article.body_text,
                    article.clean_text,
                    article.word_count as i64,
                    article.fetched_at,
                ])?;
            }
        }
        transaction.commit()?;

        let urls = articles
            .iter()
            .map(|article| article.url.as_str())
            .collect::<Vec<_>>();
        self.get_articles_by_urls(&urls)
    }

    pub fn list_articles(
        &self,
        search: Option<&str>,
        section: Option<&str>,
        only_not_uploaded: bool,
        limit: usize,
    ) -> Result<Vec<StoredArticle>> {
        let sql = if limit == 0 {
            r#"
                SELECT
                    id, url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                    source_label, body_text, clean_text, word_count, fetched_at, custom_topic, uploaded_to_lingq,
                    lingq_lesson_id, lingq_lesson_url
                FROM articles
                WHERE (?1 IS NULL OR id IN (
                    SELECT rowid FROM articles_fts WHERE articles_fts MATCH ?1
                ))
                  AND (?2 IS NULL OR section = ?2)
                  AND (?3 = 0 OR uploaded_to_lingq = 0)
                ORDER BY COALESCE(NULLIF(published_at, ''), fetched_at) DESC, fetched_at DESC
            "#
        } else {
            r#"
                SELECT
                    id, url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                    source_label, body_text, clean_text, word_count, fetched_at, custom_topic, uploaded_to_lingq,
                    lingq_lesson_id, lingq_lesson_url
                FROM articles
                WHERE (?1 IS NULL OR id IN (
                    SELECT rowid FROM articles_fts WHERE articles_fts MATCH ?1
                ))
                  AND (?2 IS NULL OR section = ?2)
                  AND (?3 = 0 OR uploaded_to_lingq = 0)
                ORDER BY COALESCE(NULLIF(published_at, ''), fetched_at) DESC, fetched_at DESC
                LIMIT ?4
            "#
        };

        let fts_query = build_fts_query(search);
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if limit == 0 {
            stmt.query_map(
                params![fts_query, section, if only_not_uploaded { 1 } else { 0 }],
                map_article_row,
            )?
        } else {
            stmt.query_map(
                params![
                    fts_query,
                    section,
                    if only_not_uploaded { 1 } else { 0 },
                    limit as i64
                ],
                map_article_row,
            )?
        };

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_article(&self, id: i64) -> Result<Option<StoredArticle>> {
        let mut stmt = self.conn.prepare_cached(
            r#"
                SELECT
                    id, url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                    source_label, body_text, clean_text, word_count, fetched_at, custom_topic, uploaded_to_lingq,
                    lingq_lesson_id, lingq_lesson_url
                FROM articles
                WHERE id = ?1
                "#,
        )?;
        stmt.query_row(params![id], map_article_row)
            .optional()
            .map_err(Into::into)
    }

    pub fn get_articles_by_ids(&self, ids: &[i64]) -> Result<Vec<StoredArticle>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                id, url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                source_label, body_text, clean_text, word_count, fetched_at, custom_topic, uploaded_to_lingq,
                lingq_lesson_id, lingq_lesson_url
            FROM articles
            WHERE id IN ({placeholders})
            "#
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), map_article_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_articles_by_urls(&self, urls: &[&str]) -> Result<Vec<StoredArticle>> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", urls.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                id, url, title, subtitle, teaser, preview_summary, author, date, published_at, section, source_kind,
                source_label, body_text, clean_text, word_count, fetched_at, custom_topic, uploaded_to_lingq,
                lingq_lesson_id, lingq_lesson_url
            FROM articles
            WHERE url IN ({placeholders})
            "#
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(urls.iter()), map_article_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_article_id_by_url(&self, url: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM articles WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_all_article_urls(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT url FROM articles")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let urls = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(urls.into_iter().collect())
    }

    pub fn mark_uploaded(&self, id: i64, lesson_id: i64, lesson_url: &str) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "UPDATE articles SET uploaded_to_lingq = 1, lingq_lesson_id = ?1, lingq_lesson_url = ?2 WHERE id = ?3",
        )?;
        stmt.execute(params![lesson_id, lesson_url, id])?;
        Ok(())
    }

    pub fn set_custom_topic(&self, id: i64, custom_topic: Option<&str>) -> Result<()> {
        let topic = custom_topic.unwrap_or("").trim().to_owned();
        self.conn.execute(
            "UPDATE articles SET custom_topic = ?1 WHERE id = ?2",
            params![topic, id],
        )?;
        Ok(())
    }

    pub fn set_custom_topic_for_articles(
        &self,
        ids: &[i64],
        custom_topic: Option<&str>,
    ) -> Result<usize> {
        let topic = custom_topic.unwrap_or("").trim().to_owned();
        let mut updated = 0usize;
        for id in ids {
            updated += self.conn.execute(
                "UPDATE articles SET custom_topic = ?1 WHERE id = ?2",
                params![topic, id],
            )?;
        }
        Ok(updated)
    }

    pub fn delete_article(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM articles WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<LibraryStats> {
        let total_articles: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM articles", [], |row| row.get(0))?;
        let uploaded_articles: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE uploaded_to_lingq = 1",
            [],
            |row| row.get(0),
        )?;
        let average_word_count = self.conn.query_row(
            "SELECT CAST(COALESCE(ROUND(AVG(word_count)), 0) AS INTEGER) FROM articles",
            [],
            |row| row.get(0),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT section, COUNT(*) FROM articles GROUP BY section ORDER BY COUNT(*) DESC, section ASC",
        )?;
        let section_rows = stmt.query_map([], |row| {
            Ok(SectionCount {
                section: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                count: row.get(1)?,
            })
        })?;
        let sections = section_rows.collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(LibraryStats {
            total_articles,
            uploaded_articles,
            average_word_count,
            sections,
        })
    }

    pub fn load_queue_snapshot(&self) -> Result<QueueSnapshot> {
        let next_job_id = self.get_app_state_u64("queue_next_job_id")?.unwrap_or(0);
        let queue_paused = self
            .get_app_state("queue_paused")?
            .is_some_and(|value| value == "1");

        let queued_jobs =
            self.load_json_list("SELECT payload FROM job_queue ORDER BY queue_position ASC")?;
        let completed_jobs = self
            .load_json_list("SELECT payload FROM completed_jobs ORDER BY completed_position ASC")?;
        let failed_fetches =
            self.load_json_list("SELECT payload FROM failed_fetches ORDER BY item_position ASC")?;
        let failed_uploads =
            self.load_json_list("SELECT payload FROM failed_uploads ORDER BY item_position ASC")?;

        Ok(QueueSnapshot {
            next_job_id,
            queue_paused,
            queued_jobs,
            completed_jobs,
            failed_fetches,
            failed_uploads,
        })
    }

    pub fn save_queue_snapshot(&mut self, snapshot: &QueueSnapshot) -> Result<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute("DELETE FROM job_queue", [])?;
        transaction.execute("DELETE FROM completed_jobs", [])?;
        transaction.execute("DELETE FROM failed_fetches", [])?;
        transaction.execute("DELETE FROM failed_uploads", [])?;
        transaction.execute(
            "INSERT INTO app_state(key, value) VALUES ('queue_next_job_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![snapshot.next_job_id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO app_state(key, value) VALUES ('queue_paused', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![if snapshot.queue_paused { "1" } else { "0" }],
        )?;

        for (index, job) in snapshot.queued_jobs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO job_queue(queue_position, payload) VALUES (?1, ?2)",
                params![index as i64, serde_json::to_string(job)?],
            )?;
        }
        for (index, job) in snapshot.completed_jobs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO completed_jobs(completed_position, payload) VALUES (?1, ?2)",
                params![index as i64, serde_json::to_string(job)?],
            )?;
        }
        for (index, item) in snapshot.failed_fetches.iter().enumerate() {
            transaction.execute(
                "INSERT INTO failed_fetches(item_position, payload) VALUES (?1, ?2)",
                params![index as i64, serde_json::to_string(item)?],
            )?;
        }
        for (index, item) in snapshot.failed_uploads.iter().enumerate() {
            transaction.execute(
                "INSERT INTO failed_uploads(item_position, payload) VALUES (?1, ?2)",
                params![index as i64, serde_json::to_string(item)?],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn record_completed_job_history(&self, job: &CompletedJob) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO job_history (job_id, kind, label, summary, success, recorded_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                job.id as i64,
                job.kind.label(),
                job.label,
                job.summary,
                if job.success { 1 } else { 0 },
                job.recorded_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_completed_job_history(&self, limit: usize) -> Result<Vec<CompletedJob>> {
        let limit = if limit == 0 { 50 } else { limit } as i64;
        let mut stmt = self.conn.prepare(
            r#"
            SELECT job_id, kind, label, summary, success, recorded_at
            FROM job_history
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let kind_label: String = row.get(1)?;
            let kind = match kind_label.as_str() {
                "Import" => crate::jobs::JobKind::Import,
                _ => crate::jobs::JobKind::Upload,
            };
            Ok(CompletedJob {
                id: row.get::<_, i64>(0)? as u64,
                kind,
                label: row.get(2)?,
                summary: row.get(3)?,
                success: row.get::<_, i64>(4)? != 0,
                recorded_at: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn configure_connection(&self) -> Result<()> {
        self.conn
            .busy_timeout(Duration::from_secs(5))
            .context("failed to set SQLite busy timeout")?;
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = -20000;
            "#,
        )?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        let mut needs_fts_rebuild = false;
        let mut version = self.user_version()?;

        if version < 1 {
            self.migrate_to_v1()?;
            version = 1;
            self.set_user_version(version)?;
        }
        if version < 2 {
            needs_fts_rebuild |= self.migrate_to_v2()?;
            version = 2;
            self.set_user_version(version)?;
        }
        if version < 3 {
            needs_fts_rebuild |= self.migrate_to_v3()?;
            version = 3;
            self.set_user_version(version)?;
        }
        if version < 4 {
            self.migrate_to_v4()?;
            version = 4;
            self.set_user_version(version)?;
        } else {
            self.migrate_to_v4()?;
        }
        if version < 5 {
            self.migrate_to_v5()?;
            version = 5;
            self.set_user_version(version)?;
        } else {
            self.migrate_to_v5()?;
        }
        if version < CURRENT_SCHEMA_VERSION {
            needs_fts_rebuild |= self.migrate_to_v6()?;
            version = CURRENT_SCHEMA_VERSION;
            self.set_user_version(version)?;
        } else {
            needs_fts_rebuild |= self.migrate_to_v6()?;
        }

        self.rebuild_fts_if_needed(needs_fts_rebuild)?;
        Ok(())
    }

    fn migrate_to_v1(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                subtitle TEXT NOT NULL DEFAULT '',
                author TEXT NOT NULL DEFAULT '',
                date TEXT NOT NULL DEFAULT '',
                section TEXT NOT NULL DEFAULT '',
                body_text TEXT NOT NULL,
                clean_text TEXT NOT NULL,
                word_count INTEGER NOT NULL DEFAULT 0,
                fetched_at TEXT NOT NULL,
                uploaded_to_lingq INTEGER NOT NULL DEFAULT 0,
                lingq_lesson_id INTEGER,
                lingq_lesson_url TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_articles_section ON articles(section);
            CREATE INDEX IF NOT EXISTS idx_articles_uploaded ON articles(uploaded_to_lingq);
            CREATE INDEX IF NOT EXISTS idx_articles_word_count ON articles(word_count);

            CREATE TABLE IF NOT EXISTS app_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS job_queue (
                queue_position INTEGER PRIMARY KEY,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS completed_jobs (
                completed_position INTEGER PRIMARY KEY,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS failed_fetches (
                item_position INTEGER PRIMARY KEY,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS failed_uploads (
                item_position INTEGER PRIMARY KEY,
                payload TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn migrate_to_v2(&self) -> Result<bool> {
        self.add_column_if_missing("articles", "custom_topic", "TEXT NOT NULL DEFAULT ''")
    }

    fn migrate_to_v3(&self) -> Result<bool> {
        let mut changed = false;
        changed |= self.add_column_if_missing("articles", "teaser", "TEXT NOT NULL DEFAULT ''")?;
        changed |=
            self.add_column_if_missing("articles", "published_at", "TEXT NOT NULL DEFAULT ''")?;
        changed |=
            self.add_column_if_missing("articles", "source_kind", "TEXT NOT NULL DEFAULT ''")?;
        changed |=
            self.add_column_if_missing("articles", "source_label", "TEXT NOT NULL DEFAULT ''")?;
        Ok(changed)
    }

    fn migrate_to_v4(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_articles_published_at ON articles(published_at);

            CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(
                title, subtitle, teaser, author, section, body_text, clean_text, url,
                content='articles', content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS articles_ai AFTER INSERT ON articles BEGIN
                INSERT INTO articles_fts(rowid, title, subtitle, teaser, author, section, body_text, clean_text, url)
                VALUES (new.id, new.title, new.subtitle, new.teaser, new.author, new.section, new.body_text, new.clean_text, new.url);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_ad AFTER DELETE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, subtitle, teaser, author, section, body_text, clean_text, url)
                VALUES('delete', old.id, old.title, old.subtitle, old.teaser, old.author, old.section, old.body_text, old.clean_text, old.url);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_au AFTER UPDATE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, subtitle, teaser, author, section, body_text, clean_text, url)
                VALUES('delete', old.id, old.title, old.subtitle, old.teaser, old.author, old.section, old.body_text, old.clean_text, old.url);
                INSERT INTO articles_fts(rowid, title, subtitle, teaser, author, section, body_text, clean_text, url)
                VALUES (new.id, new.title, new.subtitle, new.teaser, new.author, new.section, new.body_text, new.clean_text, new.url);
            END;
            "#,
        )?;
        Ok(())
    }

    fn migrate_to_v5(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS job_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                label TEXT NOT NULL,
                summary TEXT NOT NULL,
                success INTEGER NOT NULL DEFAULT 0,
                recorded_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_job_history_recorded_at ON job_history(recorded_at);
            "#,
        )?;
        Ok(())
    }

    fn migrate_to_v6(&self) -> Result<bool> {
        self.add_column_if_missing("articles", "preview_summary", "TEXT NOT NULL DEFAULT ''")
    }

    fn rebuild_fts_if_needed(&self, force_rebuild: bool) -> Result<()> {
        if !self.table_exists("articles_fts")? {
            return Ok(());
        }

        let article_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM articles", [], |row| row.get(0))?;
        let fts_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM articles_fts", [], |row| row.get(0))
            .unwrap_or(0);
        if force_rebuild || article_count != fts_count {
            self.conn.execute(
                "INSERT INTO articles_fts(articles_fts) VALUES('rebuild')",
                [],
            )?;
        }
        Ok(())
    }

    fn load_json_list<T>(&self, sql: &str) -> Result<Vec<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let payloads = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        payloads
            .into_iter()
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .collect()
    }

    fn get_app_state(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_app_state_u64(&self, key: &str) -> Result<Option<u64>> {
        self.get_app_state(key)?
            .map(|value| value.parse::<u64>().context("invalid persisted u64"))
            .transpose()
    }

    fn user_version(&self) -> Result<i32> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    fn set_user_version(&self, version: i32) -> Result<()> {
        self.conn
            .execute_batch(&format!("PRAGMA user_version = {version};"))?;
        Ok(())
    }

    fn add_column_if_missing(&self, table: &str, column: &str, definition: &str) -> Result<bool> {
        if self.has_column(table, column)? {
            return Ok(false);
        }

        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        Ok(true)
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let columns = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns.iter().any(|name| name == column))
    }

    fn table_exists(&self, table: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
                params![table],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .map_err(Into::into)
    }
}

fn map_article_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredArticle> {
    Ok(StoredArticle {
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
        source_kind: row.get(10)?,
        source_label: row.get(11)?,
        body_text: row.get(12)?,
        clean_text: row.get(13)?,
        word_count: row.get(14)?,
        fetched_at: row.get(15)?,
        custom_topic: row.get(16)?,
        uploaded_to_lingq: row.get::<_, i64>(17)? != 0,
        lingq_lesson_id: row.get(18)?,
        lingq_lesson_url: row.get(19)?,
    })
}

fn build_article_preview_summary(article: &Article) -> String {
    for candidate in [
        article.teaser.as_str(),
        article.subtitle.as_str(),
        article.body_text.as_str(),
    ] {
        let preview = normalize_preview_text(candidate, 220);
        if !preview.is_empty() {
            return preview;
        }
    }

    String::new()
}

fn normalize_preview_text(value: &str, max_chars: usize) -> String {
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

fn build_fts_query(search: Option<&str>) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::{CURRENT_SCHEMA_VERSION, Database};
    use crate::{
        jobs::{
            CompletedJob, FailedFetchItem, JobKind, QueueSnapshot, QueuedJob, QueuedJobRequest,
            UploadFailure,
        },
        soziopolis::Article,
    };
    use rusqlite::Connection;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_db_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("soziopolis_stats_test_{unique}.sqlite"))
    }

    fn sample_article(url: &str, title: &str, section: &str, word_count: usize) -> Article {
        Article {
            url: url.to_owned(),
            title: title.to_owned(),
            subtitle: String::new(),
            teaser: String::new(),
            author: "Test Author".to_owned(),
            date: "2026-04-18".to_owned(),
            published_at: "2026-04-18".to_owned(),
            section: section.to_owned(),
            source_kind: "section".to_owned(),
            source_label: section.to_owned(),
            body_text: "Body".to_owned(),
            clean_text: "Body".to_owned(),
            word_count,
            fetched_at: "2026-04-18T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn get_stats_rounds_average_word_count_without_type_errors() {
        let db_path = temp_db_path();
        let database = Database::open(&db_path).expect("database should open");

        database
            .save_article(&sample_article(
                "https://example.com/one",
                "One",
                "Essays",
                1001,
            ))
            .expect("first article should save");
        database
            .save_article(&sample_article(
                "https://example.com/two",
                "Two",
                "Debates",
                999,
            ))
            .expect("second article should save");

        let stats = database.get_stats().expect("stats should load");
        assert_eq!(stats.total_articles, 2);
        assert_eq!(stats.average_word_count, 1000);

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn list_articles_uses_fts_search() {
        let db_path = temp_db_path();
        let database = Database::open(&db_path).expect("database should open");

        let mut article = sample_article(
            "https://example.com/digital",
            "Digital Futures",
            "Essays",
            1200,
        );
        article.teaser = "A close look at platform sociology and data politics".to_owned();
        database
            .save_article(&article)
            .expect("article should save");

        let rows = database
            .list_articles(Some("platform sociology"), None, false, 0)
            .expect("fts search should work");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Digital Futures");

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn get_article_id_by_url_returns_existing_row() {
        let db_path = temp_db_path();
        let database = Database::open(&db_path).expect("database should open");
        let article = sample_article("https://example.com/lookup", "Lookup", "Essays", 900);
        let saved_id = database
            .save_article(&article)
            .expect("article should save");

        let looked_up_id = database
            .get_article_id_by_url("https://example.com/lookup")
            .expect("url lookup should succeed");

        assert_eq!(looked_up_id, Some(saved_id));

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn queue_snapshot_round_trips_through_sqlite() {
        let db_path = temp_db_path();
        let mut database = Database::open(&db_path).expect("database should open");
        let snapshot = QueueSnapshot {
            next_job_id: 7,
            queue_paused: true,
            queued_jobs: vec![QueuedJob {
                id: 7,
                kind: JobKind::Import,
                label: "Import 1 article(s)".to_owned(),
                total: 1,
                request: QueuedJobRequest::Import {
                    articles: vec![crate::soziopolis::ArticleSummary {
                        url: "https://example.com/article".to_owned(),
                        title: "Article".to_owned(),
                        teaser: "Teaser".to_owned(),
                        author: "Author".to_owned(),
                        date: "18.04.2026".to_owned(),
                        section: "Essay".to_owned(),
                        source_kind: crate::soziopolis::DiscoverySourceKind::Section,
                        source_label: "Essays".to_owned(),
                    }],
                },
            }],
            completed_jobs: vec![CompletedJob {
                id: 6,
                kind: JobKind::Upload,
                label: "Upload 1 article(s) to LingQ".to_owned(),
                summary: "Uploaded 1, failed 0, canceled no".to_owned(),
                success: true,
                recorded_at: "1710000000".to_owned(),
            }],
            failed_fetches: vec![FailedFetchItem {
                url: "https://example.com/missing".to_owned(),
                title: "Missing".to_owned(),
                category: "network".to_owned(),
                message: "timed out".to_owned(),
            }],
            failed_uploads: vec![UploadFailure {
                article_id: 12,
                title: "Upload failed".to_owned(),
                message: "unauthorized".to_owned(),
            }],
        };

        database
            .save_queue_snapshot(&snapshot)
            .expect("queue snapshot should save");
        let restored = database
            .load_queue_snapshot()
            .expect("queue snapshot should load");

        assert_eq!(restored.next_job_id, 7);
        assert!(restored.queue_paused);
        assert_eq!(restored.queued_jobs.len(), 1);
        assert_eq!(restored.completed_jobs.len(), 1);
        assert_eq!(restored.failed_fetches.len(), 1);
        assert_eq!(restored.failed_uploads.len(), 1);

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn legacy_database_gains_new_metadata_columns_before_indexes() {
        let db_path = temp_db_path();
        let conn = Connection::open(&db_path).expect("legacy database should open");
        conn.execute_batch(
            r#"
            CREATE TABLE articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                subtitle TEXT NOT NULL DEFAULT '',
                author TEXT NOT NULL DEFAULT '',
                date TEXT NOT NULL DEFAULT '',
                section TEXT NOT NULL DEFAULT '',
                body_text TEXT NOT NULL,
                clean_text TEXT NOT NULL,
                word_count INTEGER NOT NULL DEFAULT 0,
                fetched_at TEXT NOT NULL,
                uploaded_to_lingq INTEGER NOT NULL DEFAULT 0,
                lingq_lesson_id INTEGER,
                lingq_lesson_url TEXT NOT NULL DEFAULT '',
                custom_topic TEXT NOT NULL DEFAULT ''
            );
            INSERT INTO articles(url, title, subtitle, author, date, section, body_text, clean_text, word_count, fetched_at)
            VALUES (
                'https://example.com/legacy',
                'Legacy',
                '',
                'Author',
                '2026-04-18',
                'Essay',
                'Body',
                'Body',
                1234,
                '2026-04-18T12:00:00Z'
            );
            "#,
        )
        .expect("legacy schema should be created");
        drop(conn);

        let database = Database::open(&db_path).expect("migration should upgrade legacy schema");
        let rows = database
            .list_articles(Some("legacy"), None, false, 0)
            .expect("fts query should work after migration");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Legacy");
        assert!(
            database
                .has_column("articles", "published_at")
                .expect("published_at column check should succeed")
        );
        assert!(
            database
                .has_column("articles", "teaser")
                .expect("teaser column check should succeed")
        );
        assert!(
            database
                .has_column("articles", "source_kind")
                .expect("source_kind column check should succeed")
        );
        assert!(
            database
                .has_column("articles", "source_label")
                .expect("source_label column check should succeed")
        );

        drop(database);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn new_database_sets_schema_version_and_uses_wal() {
        let db_path = temp_db_path();
        let database = Database::open(&db_path).expect("database should open");

        let user_version: i32 = database
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version should be readable");
        let journal_mode: String = database
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal_mode should be readable");

        assert_eq!(user_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(journal_mode.to_lowercase(), "wal");

        drop(database);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(db_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(db_path.with_extension("sqlite-shm"));
    }
}
