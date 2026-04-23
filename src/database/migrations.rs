use super::*;

impl Database {
    pub(super) fn configure_connection(&self) -> Result<()> {
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

    pub(super) fn migrate(&self) -> Result<()> {
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
            version = 6;
            self.set_user_version(version)?;
        } else {
            needs_fts_rebuild |= self.migrate_to_v6()?;
        }
        if version < CURRENT_SCHEMA_VERSION {
            needs_fts_rebuild |= self.migrate_to_v7()?;
            version = 7;
            self.set_user_version(version)?;
        } else {
            needs_fts_rebuild |= self.migrate_to_v7()?;
        }
        if version < CURRENT_SCHEMA_VERSION {
            needs_fts_rebuild |= self.migrate_to_v8()?;
            version = CURRENT_SCHEMA_VERSION;
            self.set_user_version(version)?;
        } else {
            needs_fts_rebuild |= self.migrate_to_v8()?;
        }

        self.rebuild_fts_if_needed(needs_fts_rebuild)?;
        Ok(())
    }

    pub(super) fn migrate_to_v1(&self) -> Result<()> {
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

    pub(super) fn migrate_to_v2(&self) -> Result<bool> {
        self.add_column_if_missing("articles", "custom_topic", "TEXT NOT NULL DEFAULT ''")
    }

    pub(super) fn migrate_to_v3(&self) -> Result<bool> {
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

    pub(super) fn migrate_to_v4(&self) -> Result<()> {
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

    pub(super) fn migrate_to_v5(&self) -> Result<()> {
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

    pub(super) fn migrate_to_v6(&self) -> Result<bool> {
        self.add_column_if_missing("articles", "preview_summary", "TEXT NOT NULL DEFAULT ''")
    }

    pub(super) fn migrate_to_v7(&self) -> Result<bool> {
        self.conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS articles_ai;
            DROP TRIGGER IF EXISTS articles_ad;
            DROP TRIGGER IF EXISTS articles_au;
            DROP TABLE IF EXISTS articles_fts;

            CREATE VIRTUAL TABLE articles_fts USING fts5(
                title, subtitle, teaser, author, section, body_text, url,
                content='articles', content_rowid='id'
            );

            CREATE TRIGGER articles_ai AFTER INSERT ON articles BEGIN
                INSERT INTO articles_fts(rowid, title, subtitle, teaser, author, section, body_text, url)
                VALUES (new.id, new.title, new.subtitle, new.teaser, new.author, new.section, new.body_text, new.url);
            END;

            CREATE TRIGGER articles_ad AFTER DELETE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, subtitle, teaser, author, section, body_text, url)
                VALUES('delete', old.id, old.title, old.subtitle, old.teaser, old.author, old.section, old.body_text, old.url);
            END;

            CREATE TRIGGER articles_au AFTER UPDATE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, subtitle, teaser, author, section, body_text, url)
                VALUES('delete', old.id, old.title, old.subtitle, old.teaser, old.author, old.section, old.body_text, old.url);
                INSERT INTO articles_fts(rowid, title, subtitle, teaser, author, section, body_text, url)
                VALUES (new.id, new.title, new.subtitle, new.teaser, new.author, new.section, new.body_text, new.url);
            END;
            "#,
        )?;
        Ok(true)
    }

    pub(super) fn migrate_to_v8(&self) -> Result<bool> {
        let changed = self.add_column_if_missing(
            "articles",
            "content_fingerprint",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        self.conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_articles_content_fingerprint ON articles(content_fingerprint);
            "#,
        )?;
        Ok(changed)
    }

    pub(super) fn rebuild_fts_if_needed(&self, force_rebuild: bool) -> Result<()> {
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

    pub(super) fn load_json_list<T>(&self, sql: &str) -> Result<Vec<T>>
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

    pub(super) fn get_app_state(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(super) fn get_app_state_u64(&self, key: &str) -> Result<Option<u64>> {
        self.get_app_state(key)?
            .map(|value| value.parse::<u64>().context("invalid persisted u64"))
            .transpose()
    }

    pub(super) fn set_app_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_state(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub(super) fn user_version(&self) -> Result<i32> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub(super) fn set_user_version(&self, version: i32) -> Result<()> {
        self.conn
            .execute_batch(&format!("PRAGMA user_version = {version};"))?;
        Ok(())
    }

    pub(super) fn add_column_if_missing(&self, table: &str, column: &str, definition: &str) -> Result<bool> {
        if self.has_column(table, column)? {
            return Ok(false);
        }

        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        Ok(true)
    }

    pub(super) fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let columns = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns.iter().any(|name| name == column))
    }

    pub(super) fn table_exists(&self, table: &str) -> Result<bool> {
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
