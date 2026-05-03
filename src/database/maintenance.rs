use super::*;

impl Database {
    pub fn compact_storage(&mut self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            UPDATE articles SET clean_text = '' WHERE clean_text <> '';
            PRAGMA wal_checkpoint(TRUNCATE);
            VACUUM;
            "#,
        )?;
        self.set_app_state("clean_text_compacted_v1", "1")?;
        Ok(())
    }

    pub fn rebuild_search_index(&self) -> Result<()> {
        self.rebuild_fts_if_needed(true)
    }

    pub fn integrity_check(&self) -> Result<String> {
        self.conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub(super) fn backfill_preview_summaries_once(&mut self) -> Result<usize> {
        if self
            .get_app_state("preview_summary_backfill_v1")?
            .is_some_and(|value| value == "1")
        {
            return Ok(0);
        }

        let pending = {
            let mut stmt = self.conn.prepare(
                "SELECT id, teaser, subtitle, body_text FROM articles WHERE TRIM(preview_summary) = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let transaction = self.conn.transaction()?;
        {
            let mut update_stmt =
                transaction.prepare("UPDATE articles SET preview_summary = ?1 WHERE id = ?2")?;
            for (id, teaser, subtitle, body_text) in &pending {
                let preview_summary =
                    build_preview_summary_from_fields(teaser, subtitle, body_text);
                update_stmt.execute(params![preview_summary, id])?;
            }
        }
        transaction.execute(
            "INSERT INTO app_state(key, value) VALUES ('preview_summary_backfill_v1', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        transaction.commit()?;

        if !pending.is_empty() {
            self.rebuild_fts_if_needed(true)?;
        } else {
            self.set_app_state("preview_summary_backfill_v1", "1")?;
        }

        Ok(pending.len())
    }

    pub(super) fn clear_duplicate_clean_text_once(&mut self) -> Result<usize> {
        if self
            .get_app_state("clean_text_compacted_v1")?
            .is_some_and(|value| value == "1")
        {
            return Ok(0);
        }

        let changed = self.conn.execute(
            "UPDATE articles SET clean_text = '' WHERE clean_text <> ''",
            [],
        )?;
        self.set_app_state("clean_text_compacted_v1", "1")?;
        Ok(changed)
    }

    pub(super) fn backfill_fingerprints_once(&mut self) -> Result<usize> {
        if self
            .get_app_state("content_fingerprint_backfill_v1")?
            .is_some_and(|value| value == "1")
        {
            return Ok(0);
        }

        let pending = {
            let mut stmt = self.conn.prepare(
                "SELECT id, title, subtitle, author, date, body_text FROM articles WHERE TRIM(content_fingerprint) = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let transaction = self.conn.transaction()?;
        {
            let mut update_stmt = transaction
                .prepare("UPDATE articles SET content_fingerprint = ?1 WHERE id = ?2")?;
            for (id, title, subtitle, author, date, body_text) in &pending {
                let fingerprint = build_text_fingerprint(title, subtitle, author, date, body_text);
                update_stmt.execute(params![fingerprint, id])?;
            }
        }
        transaction.execute(
            "INSERT INTO app_state(key, value) VALUES ('content_fingerprint_backfill_v1', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        transaction.commit()?;
        Ok(pending.len())
    }

    pub(super) fn backfill_generated_topics_once(&mut self) -> Result<usize> {
        if self
            .get_app_state("generated_topic_backfill_v1")?
            .is_some_and(|value| value == "1")
        {
            return Ok(0);
        }

        let pending = {
            let mut stmt = self.conn.prepare(
                "SELECT id, title, subtitle, section, url FROM articles WHERE TRIM(generated_topic) = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let transaction = self.conn.transaction()?;
        {
            let mut update_stmt =
                transaction.prepare("UPDATE articles SET generated_topic = ?1 WHERE id = ?2")?;
            for (id, title, subtitle, section, url) in &pending {
                let generated_topic =
                    build_generated_topic_from_fields(title, subtitle, section, url);
                update_stmt.execute(params![generated_topic, id])?;
            }
        }
        transaction.execute(
            "INSERT INTO app_state(key, value) VALUES ('generated_topic_backfill_v1', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        transaction.commit()?;

        if !pending.is_empty() {
            self.rebuild_fts_if_needed(true)?;
        } else {
            self.set_app_state("generated_topic_backfill_v1", "1")?;
        }

        Ok(pending.len())
    }
}
