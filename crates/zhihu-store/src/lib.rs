//! SQLite persistence: settings, accounts, cookies, cache, history, blocklists.
//!
//! See `docs/REWRITE_PLAN.md` §5.4.
//!
//! **Security note:** cookies are currently stored as plaintext. Before a
//! production release they must be encrypted with a platform keystore
//! (Android Keystore / desktop keyring); see M3.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};
use zhihu_recommend::{Candidate, CandidateKind, FeedbackRecord, RecommendPoolStats};
use zhihu_types::{Highlight, HistoryEntry};

pub const DB_FILE_NAME: &str = "zhihu-rust.sqlite3";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("sqlite mutex poisoned")
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn save_account(
        &self,
        id: i64,
        name: &str,
        user_token: Option<&str>,
        is_current: bool,
    ) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        if is_current {
            tx.execute("UPDATE accounts SET is_current = 0", [])?;
        }
        tx.execute(
            "INSERT INTO accounts(id, name, user_token, is_current) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               user_token = excluded.user_token,
               is_current = excluded.is_current",
            params![id, name, user_token, is_current as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn current_account(&self) -> Result<Option<(i64, String)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, name FROM accounts WHERE is_current = 1 ORDER BY id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn save_cookies(&self, account_id: i64, cookies: &[(String, String)]) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cookies WHERE account_id = ?1", [account_id])?;
        {
            let mut statement =
                tx.prepare("INSERT INTO cookies(account_id, name, value) VALUES(?1, ?2, ?3)")?;
            for (name, value) in cookies {
                statement.execute(params![account_id, name, value])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_cookies(&self, account_id: i64) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT name, value FROM cookies WHERE account_id = ?1 ORDER BY name")?;
        let rows = statement.query_map([account_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn add_block_keyword(&self, pattern: &str, is_regex: bool) -> Result<()> {
        self.conn().execute(
            "INSERT INTO block_keywords(pattern, is_regex, enabled) VALUES(?1, ?2, 1)
             ON CONFLICT(pattern) DO UPDATE SET is_regex = excluded.is_regex, enabled = 1",
            params![pattern, is_regex as i64],
        )?;
        Ok(())
    }

    pub fn remove_block_keyword(&self, pattern: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM block_keywords WHERE pattern = ?1", [pattern])?;
        Ok(())
    }

    pub fn block_keywords(&self) -> Result<Vec<(String, bool)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT pattern, is_regex FROM block_keywords WHERE enabled = 1 ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn add_block_user(&self, id: &str, name: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO block_users(id, name) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn remove_block_user(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM block_users WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn blocked_users(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT id, name FROM block_users ORDER BY name")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn add_block_topic(&self, id: &str, name: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO block_topics(id, name) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn remove_block_topic(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM block_topics WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn blocked_topics(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT id, name FROM block_topics ORDER BY name")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn add_recommend_hidden_user(&self, id: &str, name: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO recommend_hidden_users(id, name) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn remove_recommend_hidden_user(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM recommend_hidden_users WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn recommend_hidden_users(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT id, name FROM recommend_hidden_users")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_highlight(
        &self,
        target_type: &str,
        target_id: &str,
        title: &str,
        quote: &str,
        note: &str,
        tags: &str,
        color: &str,
        start_offset: i64,
        end_offset: i64,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO highlights(
                target_type, target_id, title, quote, note, tags, color,
                start_offset, end_offset, created_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                target_type,
                target_id,
                title,
                quote,
                note,
                tags,
                color,
                start_offset,
                end_offset,
                created_at
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn highlights(&self, limit: usize) -> Result<Vec<Highlight>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT id, target_type, target_id, title, quote, note, tags, color,
                    start_offset, end_offset, created_at
             FROM highlights ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| {
            Ok(Highlight {
                id: row.get(0)?,
                target_type: row.get(1)?,
                target_id: row.get(2)?,
                title: row.get(3)?,
                quote: row.get(4)?,
                note: row.get(5)?,
                tags: row.get(6)?,
                color: row.get(7)?,
                start_offset: row.get(8)?,
                end_offset: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_highlight(&self, id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM highlights WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_highlights(&self) -> Result<()> {
        self.conn().execute("DELETE FROM highlights", [])?;
        Ok(())
    }

    pub fn add_search_history(&self, query: &str, searched_at: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO search_history(query, searched_at) VALUES(?1, ?2)
             ON CONFLICT(query) DO UPDATE SET searched_at = excluded.searched_at",
            params![query, searched_at],
        )?;
        Ok(())
    }

    pub fn record_read(
        &self,
        target_type: &str,
        target_id: &str,
        title: &str,
        read_at: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO read_history(target_type, target_id, title, read_at, progress)
             VALUES(?1, ?2, ?3, ?4, 0)
             ON CONFLICT(target_type, target_id) DO UPDATE SET
                title = excluded.title,
                read_at = excluded.read_at",
            params![target_type, target_id, title, read_at],
        )?;
        Ok(())
    }

    pub fn update_read_progress(
        &self,
        target_type: &str,
        target_id: &str,
        progress: f32,
        read_at: i64,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE read_history SET progress = ?1, read_at = ?2
             WHERE target_type = ?3 AND target_id = ?4",
            params![progress.clamp(0.0, 1.0), read_at, target_type, target_id],
        )?;
        Ok(())
    }

    pub fn read_progress(&self, target_type: &str, target_id: &str) -> Result<f32> {
        Ok(self
            .conn()
            .query_row(
                "SELECT progress FROM read_history
                 WHERE target_type = ?1 AND target_id = ?2",
                params![target_type, target_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0.0))
    }

    pub fn cache_content(
        &self,
        target_type: &str,
        target_id: &str,
        payload: &str,
        fetched_at: i64,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO content_cache(target_type, target_id, payload, fetched_at)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(target_type, target_id) DO UPDATE SET
                payload = excluded.payload,
                fetched_at = excluded.fetched_at",
            params![target_type, target_id, payload, fetched_at],
        )?;
        conn.execute(
            "DELETE FROM content_cache WHERE rowid NOT IN (
                SELECT rowid FROM content_cache ORDER BY fetched_at DESC LIMIT 200
             )",
            [],
        )?;
        Ok(())
    }

    pub fn cached_content(
        &self,
        target_type: &str,
        target_id: &str,
    ) -> Result<Option<(String, i64)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT payload, fetched_at FROM content_cache
                 WHERE target_type = ?1 AND target_id = ?2",
                params![target_type, target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn cache_feed(&self, mode: &str, payload: &str, fetched_at: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM feed_cache WHERE account_id = 0", [])?;
        conn.execute(
            "INSERT INTO feed_cache(account_id, mode, payload, fetched_at)
             VALUES(0, ?1, ?2, ?3)",
            params![mode, payload, fetched_at],
        )?;
        Ok(())
    }

    pub fn cached_feed(&self, mode: &str) -> Result<Option<(String, i64)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT payload, fetched_at FROM feed_cache
                 WHERE account_id = 0 AND mode = ?1
                 ORDER BY fetched_at DESC LIMIT 1",
                [mode],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn read_history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT target_type, target_id, title, read_at, progress
             FROM read_history ORDER BY read_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| {
            Ok(HistoryEntry {
                kind: row.get(0)?,
                id: row.get(1)?,
                title: row.get(2)?,
                read_at: row.get(3)?,
                progress: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn().execute("DELETE FROM read_history", [])?;
        Ok(())
    }

    pub fn search_history(&self, limit: usize) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT query FROM search_history ORDER BY searched_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit as i64], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn clear_search_history(&self) -> Result<()> {
        self.conn().execute("DELETE FROM search_history", [])?;
        Ok(())
    }

    /// Record explicit on-device interest. Invalid JSON in `topics` is stored
    /// as an empty list rather than failing the interaction.
    pub fn record_recommend_feedback(
        &self,
        item_id: &str,
        action: &str,
        author_id: &str,
        topics: &[String],
        weight: f32,
        created_at: i64,
    ) -> Result<()> {
        let topics_json = serde_json::to_string(topics).unwrap_or_else(|_| "[]".to_string());
        let conn = self.conn();
        conn.execute(
            "INSERT INTO recommend_feedback(item_id, action, author_id, topics, weight, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(item_id, action) DO UPDATE SET
                author_id = excluded.author_id,
                topics = excluded.topics,
                weight = MAX(recommend_feedback.weight, excluded.weight),
                created_at = excluded.created_at",
            params![item_id, action, author_id, topics_json, weight, created_at],
        )?;
        // Bound local storage; the recommender only needs recent behaviour.
        conn.execute(
            "DELETE FROM recommend_feedback
             WHERE rowid NOT IN (
                SELECT rowid FROM recommend_feedback ORDER BY created_at DESC LIMIT 2000
             )",
            [],
        )?;
        Ok(())
    }

    pub fn recommend_feedback(&self, limit: usize) -> Result<Vec<FeedbackRecord>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT author_id, topics, weight, created_at
             FROM recommend_feedback ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| {
            let author_id: String = row.get(0)?;
            let topics_json: String = row.get(1)?;
            Ok(FeedbackRecord {
                author_id,
                topics: serde_json::from_str(&topics_json).unwrap_or_default(),
                weight: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Raw `(item_id, weight, created_at)` signals, used to build the vector
    /// interest center. Kept separate from [`Self::recommend_feedback`] so the
    /// public feedback record stays unchanged.
    pub fn recommend_feedback_signals(&self, limit: usize) -> Result<Vec<(String, f32, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT item_id, weight, created_at FROM recommend_feedback
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn recommend_feedback_count(&self) -> Result<usize> {
        Ok(self
            .conn()
            .query_row("SELECT COUNT(*) FROM recommend_feedback", [], |row| {
                row.get::<_, i64>(0).map(|count| count as usize)
            })?)
    }

    pub fn disliked_item_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT item_id FROM recommend_feedback WHERE action = 'dislike'")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn clear_recommend_feedback(&self) -> Result<()> {
        self.conn().execute("DELETE FROM recommend_feedback", [])?;
        Ok(())
    }

    /// Insert collected candidates, deduplicating on `(kind, id)`.
    ///
    /// Re-seeing an item refreshes its metadata and expiry but keeps the
    /// original `collected_at`, so pool ordering reflects first discovery.
    /// Returns `(inserted, updated)`.
    pub fn upsert_recommend_candidates(&self, candidates: &[Candidate]) -> Result<(usize, usize)> {
        if candidates.is_empty() {
            return Ok((0, 0));
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let (mut inserted, mut updated) = (0usize, 0usize);
        {
            let mut exists = tx.prepare(
                "SELECT EXISTS(SELECT 1 FROM recommend_pool WHERE kind = ?1 AND id = ?2)",
            )?;
            let mut upsert = tx.prepare(
                "INSERT INTO recommend_pool(
                    kind, id, title, excerpt, author_id, author_name, topics,
                    vote_up_count, comment_count, is_mcn, source,
                    collected_at, expires_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(kind, id) DO UPDATE SET
                    title = excluded.title,
                    excerpt = excluded.excerpt,
                    author_id = excluded.author_id,
                    author_name = excluded.author_name,
                    topics = excluded.topics,
                    vote_up_count = excluded.vote_up_count,
                    comment_count = excluded.comment_count,
                    is_mcn = excluded.is_mcn,
                    source = excluded.source,
                    expires_at = MAX(recommend_pool.expires_at, excluded.expires_at)",
            )?;
            for candidate in candidates {
                let kind = candidate.kind.as_str();
                let topics =
                    serde_json::to_string(&candidate.topics).unwrap_or_else(|_| "[]".to_string());
                let existed: bool =
                    exists.query_row(params![kind, candidate.id], |row| row.get(0))?;
                upsert.execute(params![
                    kind,
                    candidate.id,
                    candidate.title,
                    candidate.excerpt,
                    candidate.author_id,
                    candidate.author_name,
                    topics,
                    candidate.vote_up_count as i64,
                    candidate.comment_count as i64,
                    candidate.is_mcn as i64,
                    candidate.source,
                    candidate.collected_at,
                    candidate.expires_at,
                ])?;
                if existed {
                    updated += 1;
                } else {
                    inserted += 1;
                }
            }
        }
        tx.commit()?;
        Ok((inserted, updated))
    }

    /// Non-expired pool entries, newest first.
    pub fn recommend_pool(&self, limit: usize, now: i64) -> Result<Vec<Candidate>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT kind, id, title, excerpt, author_id, author_name, topics,
                    vote_up_count, comment_count, is_mcn, source, collected_at, expires_at
             FROM recommend_pool WHERE expires_at > ?1
             ORDER BY collected_at DESC, kind, id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![now, limit as i64], candidate_from_row)?;
        let mut items = Vec::new();
        for row in rows {
            if let Some(candidate) = row? {
                items.push(candidate);
            }
        }
        Ok(items)
    }

    pub fn recommend_pool_count(&self, now: i64) -> Result<usize> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM recommend_pool WHERE expires_at > ?1",
            [now],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?)
    }

    pub fn recommend_pool_stats(&self, now: i64) -> Result<RecommendPoolStats> {
        let (items, oldest, newest, bytes): (i64, Option<i64>, Option<i64>, i64) =
            self.conn().query_row(
                "SELECT COUNT(*), MIN(collected_at), MAX(collected_at),
                        COALESCE(SUM(LENGTH(title) + LENGTH(excerpt) + LENGTH(author_id)
                            + LENGTH(author_name) + LENGTH(topics) + LENGTH(source) + 64), 0)
                 FROM recommend_pool WHERE expires_at > ?1",
                [now],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        Ok(RecommendPoolStats {
            items: items as usize,
            oldest_collected_at: oldest,
            newest_collected_at: newest,
            approx_bytes: bytes.max(0) as u64,
        })
    }

    /// Drop expired entries, then the oldest entries beyond `max_items`.
    /// Returns the number of removed rows.
    pub fn prune_recommend_pool(&self, now: i64, max_items: usize) -> Result<usize> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut removed = tx.execute("DELETE FROM recommend_pool WHERE expires_at <= ?1", [now])?;
        removed += tx.execute(
            "DELETE FROM recommend_pool WHERE rowid NOT IN (
                SELECT rowid FROM recommend_pool
                ORDER BY collected_at DESC, kind, id LIMIT ?1
             )",
            [max_items.max(1) as i64],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn clear_recommend_pool(&self) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM recommend_pool", [])?;
        tx.execute("DELETE FROM recommend_embedding", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Cache embeddings for one model. `(content_key, model_id)` is the key,
    /// so a provider upgrade never mixes vectors from two models.
    pub fn upsert_recommend_embeddings(
        &self,
        model_id: &str,
        entries: &[(String, Vec<f32>)],
        generated_at: i64,
    ) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut statement = tx.prepare(
                "INSERT INTO recommend_embedding(
                    content_key, model_id, dimension, vector, generated_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(content_key, model_id) DO UPDATE SET
                    dimension = excluded.dimension,
                    vector = excluded.vector,
                    generated_at = excluded.generated_at",
            )?;
            for (key, vector) in entries {
                statement.execute(params![
                    key,
                    model_id,
                    vector.len() as i64,
                    vector_to_blob(vector),
                    generated_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn recommend_embeddings(&self, model_id: &str) -> Result<Vec<(String, Vec<f32>)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT content_key, vector FROM recommend_embedding WHERE model_id = ?1
             ORDER BY content_key",
        )?;
        let rows = statement.query_map([model_id], |row| {
            let key: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((key, blob_to_vector(&blob)))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn recommend_embedding_count(&self, model_id: &str) -> Result<usize> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM recommend_embedding WHERE model_id = ?1",
            [model_id],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?)
    }

    /// Drop embeddings from every other model version.
    pub fn prune_recommend_embeddings(&self, keep_model: &str) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM recommend_embedding WHERE model_id <> ?1",
            [keep_model],
        )?)
    }

    pub fn clear_recommend_embeddings(&self) -> Result<()> {
        self.conn().execute("DELETE FROM recommend_embedding", [])?;
        Ok(())
    }
}

fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

fn candidate_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Candidate>> {
    let raw_kind: String = row.get(0)?;
    let Some(kind) = CandidateKind::parse(&raw_kind) else {
        return Ok(None);
    };
    let topics_json: String = row.get(6)?;
    Ok(Some(Candidate {
        kind,
        id: row.get(1)?,
        title: row.get(2)?,
        excerpt: row.get(3)?,
        author_id: row.get(4)?,
        author_name: row.get(5)?,
        topics: serde_json::from_str(&topics_json).unwrap_or_default(),
        vote_up_count: row.get::<_, i64>(7)?.max(0) as u64,
        comment_count: row.get::<_, i64>(8)?.max(0) as u64,
        is_mcn: row.get::<_, i64>(9)? != 0,
        source: row.get(10)?,
        collected_at: row.get(11)?,
        expires_at: row.get(12)?,
    }))
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS accounts (
            id          INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            user_token  TEXT,
            is_current  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS cookies (
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            name       TEXT NOT NULL,
            value      TEXT NOT NULL,
            PRIMARY KEY (account_id, name)
        );

        CREATE TABLE IF NOT EXISTS feed_cache (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER,
            mode       TEXT NOT NULL,
            cursor     TEXT,
            payload    TEXT NOT NULL,
            fetched_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS content_cache (
            target_type TEXT NOT NULL,
            target_id   TEXT NOT NULL,
            payload     TEXT NOT NULL,
            fetched_at  INTEGER NOT NULL,
            PRIMARY KEY (target_type, target_id)
        );

        CREATE TABLE IF NOT EXISTS read_history (
            target_type TEXT NOT NULL,
            target_id   TEXT NOT NULL,
            title       TEXT NOT NULL,
            read_at     INTEGER NOT NULL,
            progress    REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (target_type, target_id)
        );

        CREATE TABLE IF NOT EXISTS search_history (
            query       TEXT PRIMARY KEY,
            searched_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS block_keywords (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            pattern  TEXT NOT NULL UNIQUE,
            is_regex INTEGER NOT NULL DEFAULT 0,
            enabled  INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS block_users (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS block_topics (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS drafts (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            target_type TEXT NOT NULL,
            target_id   TEXT,
            body        TEXT NOT NULL,
            updated_at  INTEGER NOT NULL
        );

        PRAGMA user_version = 1;
        COMMIT;
        "#,
        )?;
    }

    if version < 2 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS recommend_feedback (
            item_id    TEXT NOT NULL,
            action     TEXT NOT NULL,
            author_id  TEXT NOT NULL DEFAULT '',
            topics     TEXT NOT NULL DEFAULT '[]',
            weight     REAL NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (item_id, action)
        );
        CREATE INDEX IF NOT EXISTS idx_recommend_feedback_created
            ON recommend_feedback(created_at DESC);
        PRAGMA user_version = 2;
        COMMIT;
        "#,
        )?;
    }

    if version < 3 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS recommend_hidden_users (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT ''
        );
        PRAGMA user_version = 3;
        COMMIT;
        "#,
        )?;
    }

    if version < 4 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS highlights (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            target_type  TEXT NOT NULL,
            target_id    TEXT NOT NULL,
            title        TEXT NOT NULL DEFAULT '',
            quote        TEXT NOT NULL,
            note         TEXT NOT NULL DEFAULT '',
            tags         TEXT NOT NULL DEFAULT '',
            color        TEXT NOT NULL DEFAULT 'yellow',
            start_offset INTEGER NOT NULL DEFAULT 0,
            end_offset   INTEGER NOT NULL DEFAULT 0,
            created_at   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_highlights_target
            ON highlights(target_type, target_id, start_offset);
        CREATE INDEX IF NOT EXISTS idx_highlights_created
            ON highlights(created_at DESC);
        PRAGMA user_version = 4;
        COMMIT;
        "#,
        )?;
    }

    if version < 5 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS recommend_pool (
            kind          TEXT NOT NULL,
            id            TEXT NOT NULL,
            title         TEXT NOT NULL DEFAULT '',
            excerpt       TEXT NOT NULL DEFAULT '',
            author_id     TEXT NOT NULL DEFAULT '',
            author_name   TEXT NOT NULL DEFAULT '',
            topics        TEXT NOT NULL DEFAULT '[]',
            vote_up_count INTEGER NOT NULL DEFAULT 0,
            comment_count INTEGER NOT NULL DEFAULT 0,
            is_mcn        INTEGER NOT NULL DEFAULT 0,
            source        TEXT NOT NULL,
            collected_at  INTEGER NOT NULL,
            expires_at    INTEGER NOT NULL,
            PRIMARY KEY (kind, id)
        );
        CREATE INDEX IF NOT EXISTS idx_recommend_pool_collected
            ON recommend_pool(collected_at DESC);
        CREATE INDEX IF NOT EXISTS idx_recommend_pool_expires
            ON recommend_pool(expires_at);
        PRAGMA user_version = 5;
        COMMIT;
        "#,
        )?;
    }

    if version < 6 {
        conn.execute_batch(
            r#"
        BEGIN;
        CREATE TABLE IF NOT EXISTS recommend_embedding (
            content_key  TEXT NOT NULL,
            model_id     TEXT NOT NULL,
            dimension    INTEGER NOT NULL,
            vector       BLOB NOT NULL,
            generated_at INTEGER NOT NULL,
            PRIMARY KEY (content_key, model_id)
        );
        CREATE INDEX IF NOT EXISTS idx_recommend_embedding_model
            ON recommend_embedding(model_id);
        PRAGMA user_version = 6;
        COMMIT;
        "#,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.get_setting("theme").unwrap(), None);
        store.set_setting("theme", "dark").unwrap();
        assert_eq!(store.get_setting("theme").unwrap().as_deref(), Some("dark"));
        store.set_setting("theme", "light").unwrap();
        assert_eq!(
            store.get_setting("theme").unwrap().as_deref(),
            Some("light")
        );
    }

    #[test]
    fn cookies_roundtrip_and_replace() {
        let store = Store::open_in_memory().unwrap();
        store.save_account(1, "tester", Some("tok"), true).unwrap();
        store
            .save_cookies(
                1,
                &[
                    ("z_c0".to_string(), "a".to_string()),
                    ("d_c0".to_string(), "b".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(store.load_cookies(1).unwrap().len(), 2);

        store
            .save_cookies(1, &[("z_c0".to_string(), "c".to_string())])
            .unwrap();
        let cookies = store.load_cookies(1).unwrap();
        assert_eq!(cookies, vec![("z_c0".to_string(), "c".to_string())]);
    }

    #[test]
    fn only_one_current_account() {
        let store = Store::open_in_memory().unwrap();
        store.save_account(1, "one", None, true).unwrap();
        store.save_account(2, "two", None, true).unwrap();
        assert_eq!(store.current_account().unwrap().unwrap().0, 2);
    }

    #[test]
    fn block_keywords_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        store.add_block_keyword("广告", false).unwrap();
        store.add_block_keyword("^推广", true).unwrap();
        let keywords = store.block_keywords().unwrap();
        assert_eq!(keywords.len(), 2);
        store.remove_block_keyword("广告").unwrap();
        assert_eq!(store.block_keywords().unwrap().len(), 1);
    }

    #[test]
    fn read_history_is_upserted_and_cleared() {
        let store = Store::open_in_memory().unwrap();
        store.record_read("answer", "1", "first", 100).unwrap();
        store
            .record_read("answer", "1", "first again", 200)
            .unwrap();
        store.record_read("article", "2", "second", 150).unwrap();

        let history = store.read_history(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "1");
        assert_eq!(history[0].title, "first again");
        assert_eq!(history[1].kind, "article");

        store.clear_history().unwrap();
        assert!(store.read_history(10).unwrap().is_empty());
    }

    #[test]
    fn reading_progress_updates_without_losing_history() {
        let store = Store::open_in_memory().unwrap();
        store.record_read("pin", "9", "想法", 100).unwrap();
        assert_eq!(store.read_progress("pin", "9").unwrap(), 0.0);

        store.update_read_progress("pin", "9", 0.42, 200).unwrap();
        store.record_read("pin", "9", "想法", 300).unwrap();
        assert!((store.read_progress("pin", "9").unwrap() - 0.42).abs() < 0.001);
        assert_eq!(store.read_history(10).unwrap()[0].read_at, 300);

        store.update_read_progress("pin", "9", 3.0, 400).unwrap();
        assert_eq!(store.read_progress("pin", "9").unwrap(), 1.0);
        assert_eq!(store.read_progress("pin", "missing").unwrap(), 0.0);
    }

    #[test]
    fn caches_content_and_feed_snapshots() {
        let store = Store::open_in_memory().unwrap();
        store
            .cache_content("answer", "1", r#"{"title":"a"}"#, 100)
            .unwrap();
        assert_eq!(
            store.cached_content("answer", "1").unwrap(),
            Some((r#"{"title":"a"}"#.to_string(), 100))
        );
        store
            .cache_content("answer", "1", r#"{"title":"b"}"#, 200)
            .unwrap();
        assert_eq!(
            store.cached_content("answer", "1").unwrap().unwrap().0,
            r#"{"title":"b"}"#
        );

        store.cache_feed("recommend", "[]", 100).unwrap();
        store.cache_feed("recommend", "[1]", 200).unwrap();
        assert_eq!(
            store.cached_feed("recommend").unwrap(),
            Some(("[1]".to_string(), 200))
        );
        assert!(store.cached_feed("follow").unwrap().is_none());
    }

    #[test]
    fn recommendation_hidden_users_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        store.add_recommend_hidden_user("u1", "alice").unwrap();
        store.add_recommend_hidden_user("u1", "alice2").unwrap();
        assert_eq!(
            store.recommend_hidden_users().unwrap(),
            vec![("u1".to_string(), "alice2".to_string())]
        );
        store.remove_recommend_hidden_user("u1").unwrap();
        assert!(store.recommend_hidden_users().unwrap().is_empty());
    }

    #[test]
    fn highlights_roundtrip_and_clear() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_highlight(
                "answer",
                "1",
                "标题",
                "好句子",
                "我的想法",
                "Rust,阅读",
                "yellow",
                3,
                6,
                100,
            )
            .unwrap();
        let items = store.highlights(10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].quote, "好句子");
        assert_eq!(items[0].note, "我的想法");
        store.delete_highlight(id).unwrap();
        assert!(store.highlights(10).unwrap().is_empty());
        store
            .add_highlight("pin", "2", "想法", "摘录", "", "", "green", 0, 2, 200)
            .unwrap();
        store.clear_highlights().unwrap();
        assert!(store.highlights(10).unwrap().is_empty());
    }

    #[test]
    fn search_history_is_deduplicated() {
        let store = Store::open_in_memory().unwrap();
        store.add_search_history("rust", 1).unwrap();
        store.add_search_history("gpui", 2).unwrap();
        store.add_search_history("rust", 3).unwrap();
        assert_eq!(store.search_history(10).unwrap(), vec!["rust", "gpui"]);
        store.clear_search_history().unwrap();
        assert!(store.search_history(10).unwrap().is_empty());
    }

    #[test]
    fn recommend_pool_deduplicates_and_keeps_first_seen() {
        let store = Store::open_in_memory().unwrap();
        let mut first = pool_candidate("1", CandidateKind::Answer, 100, 100 + 7 * 86_400);
        first.title = "旧标题".to_string();
        assert_eq!(
            store.upsert_recommend_candidates(&[first.clone()]).unwrap(),
            (1, 0)
        );

        let mut again = first;
        again.title = "新标题".to_string();
        again.vote_up_count = 99;
        again.source = "topic:7".to_string();
        again.expires_at = 200 + 7 * 86_400;
        assert_eq!(store.upsert_recommend_candidates(&[again]).unwrap(), (0, 1));

        let pool = store.recommend_pool(10, 150).unwrap();
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].title, "新标题");
        assert_eq!(pool[0].vote_up_count, 99);
        assert_eq!(pool[0].source, "topic:7");
        assert_eq!(pool[0].collected_at, 100);
        assert_eq!(pool[0].expires_at, 200 + 7 * 86_400);
        assert_eq!(pool[0].topics, vec!["Rust".to_string()]);
    }

    #[test]
    fn recommend_pool_prunes_expired_and_overflow() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_recommend_candidates(&[
                pool_candidate("expired", CandidateKind::Answer, 10, 50),
                pool_candidate("fresh", CandidateKind::Article, 400, 10_000),
                pool_candidate("middle", CandidateKind::Pin, 300, 10_000),
            ])
            .unwrap();

        assert_eq!(store.recommend_pool_count(100).unwrap(), 2);
        assert_eq!(store.prune_recommend_pool(100, 10).unwrap(), 1);
        assert_eq!(store.recommend_pool_count(100).unwrap(), 2);

        // The oldest remaining entry is evicted first.
        assert_eq!(store.prune_recommend_pool(100, 1).unwrap(), 1);
        let pool = store.recommend_pool(10, 100).unwrap();
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].id, "fresh");

        store
            .upsert_recommend_candidates(&[pool_candidate(
                "expired-later",
                CandidateKind::Question,
                10,
                200,
            )])
            .unwrap();
        assert_eq!(store.prune_recommend_pool(300, 10).unwrap(), 1);
        assert_eq!(store.recommend_pool_count(300).unwrap(), 1);
    }

    #[test]
    fn recommend_pool_stats_and_clear() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_recommend_candidates(&[
                pool_candidate("1", CandidateKind::Answer, 100, 10_000),
                pool_candidate("2", CandidateKind::Pin, 200, 10_000),
            ])
            .unwrap();
        let stats = store.recommend_pool_stats(1_000).unwrap();
        assert_eq!(stats.items, 2);
        assert_eq!(stats.oldest_collected_at, Some(100));
        assert_eq!(stats.newest_collected_at, Some(200));
        assert!(stats.approx_bytes > 0);

        store.clear_recommend_pool().unwrap();
        assert_eq!(store.recommend_pool_count(1_000).unwrap(), 0);
        assert_eq!(
            store.recommend_pool_stats(1_000).unwrap(),
            Default::default()
        );
    }

    #[test]
    fn recommend_embeddings_roundtrip_and_prune() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_recommend_embeddings(
                "hash-bigram-v1",
                &[
                    ("answer:1".to_string(), vec![0.5, -0.25, 0.125]),
                    ("answer:2".to_string(), vec![1.0, 0.0]),
                ],
                100,
            )
            .unwrap();
        let rows = store.recommend_embeddings("hash-bigram-v1").unwrap();
        assert_eq!(rows.len(), 2);
        let first = rows.iter().find(|(key, _)| key == "answer:1").unwrap();
        assert_eq!(first.1, vec![0.5, -0.25, 0.125]);
        assert_eq!(
            store.recommend_embedding_count("hash-bigram-v1").unwrap(),
            2
        );

        // Re-embedding replaces the vector instead of duplicating the row.
        store
            .upsert_recommend_embeddings(
                "hash-bigram-v1",
                &[("answer:1".to_string(), vec![2.0])],
                200,
            )
            .unwrap();
        let rows = store.recommend_embeddings("hash-bigram-v1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter().find(|(key, _)| key == "answer:1").unwrap().1,
            vec![2.0]
        );

        store
            .upsert_recommend_embeddings("other", &[("answer:3".to_string(), vec![1.0])], 300)
            .unwrap();
        assert_eq!(
            store.prune_recommend_embeddings("hash-bigram-v1").unwrap(),
            1
        );
        assert_eq!(store.recommend_embedding_count("other").unwrap(), 0);

        store.clear_recommend_embeddings().unwrap();
        assert_eq!(
            store.recommend_embedding_count("hash-bigram-v1").unwrap(),
            0
        );
    }

    #[test]
    fn clearing_pool_drops_embeddings_too() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_recommend_candidates(&[pool_candidate("1", CandidateKind::Answer, 100, 10_000)])
            .unwrap();
        store
            .upsert_recommend_embeddings("m", &[("answer:1".to_string(), vec![1.0])], 100)
            .unwrap();
        store.clear_recommend_pool().unwrap();
        assert_eq!(store.recommend_pool_count(1_000).unwrap(), 0);
        assert_eq!(store.recommend_embedding_count("m").unwrap(), 0);
    }

    fn pool_candidate(
        id: &str,
        kind: CandidateKind,
        collected_at: i64,
        expires_at: i64,
    ) -> Candidate {
        Candidate {
            kind,
            id: id.to_string(),
            title: format!("标题{id}"),
            excerpt: "摘要".to_string(),
            author_id: "u1".to_string(),
            author_name: "alice".to_string(),
            topics: vec!["Rust".to_string()],
            vote_up_count: 1,
            comment_count: 0,
            is_mcn: false,
            source: "recommend".to_string(),
            collected_at,
            expires_at,
        }
    }

    #[test]
    fn recommend_feedback_upserts_and_clears() {
        let store = Store::open_in_memory().unwrap();
        let topics = vec!["Rust".to_string(), "GPUI".to_string()];
        store
            .record_recommend_feedback("answer:1", "read", "author-1", &topics, 1.0, 100)
            .unwrap();
        store
            .record_recommend_feedback("answer:1", "read", "author-1", &topics, 3.0, 200)
            .unwrap();
        store
            .record_recommend_feedback("answer:1", "like", "author-1", &topics, 2.0, 300)
            .unwrap();
        store
            .record_recommend_feedback("answer:2", "dislike", "author-2", &topics, -1.5, 400)
            .unwrap();

        assert_eq!(store.recommend_feedback_count().unwrap(), 3);
        assert_eq!(
            store.disliked_item_ids().unwrap(),
            vec!["answer:2".to_string()]
        );
        let rows = store.recommend_feedback(10).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].created_at, 400);
        assert_eq!(rows[0].weight, -1.5);
        assert_eq!(rows[1].topics, topics);
        assert_eq!(rows[2].weight, 3.0);

        store.clear_recommend_feedback().unwrap();
        assert_eq!(store.recommend_feedback_count().unwrap(), 0);
    }
}
