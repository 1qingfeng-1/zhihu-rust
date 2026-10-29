//! SQLite persistence: settings, accounts, cookies, cache, history, blocklists.
//!
//! See `docs/REWRITE_PLAN.md` §5.4.
//!
//! **Security note:** cookies are currently stored as plaintext. Before a
//! production release they must be encrypted with a platform keystore
//! (Android Keystore / desktop keyring); see M3.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use zhihu_recommend::FeedbackRecord;
use zhihu_types::HistoryEntry;

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
    fn search_history_is_deduplicated() {
        let store = Store::open_in_memory().unwrap();
        store.add_search_history("rust", 1).unwrap();
        store.add_search_history("gpui", 2).unwrap();
        store.add_search_history("rust", 3).unwrap();
        assert_eq!(store.search_history(10).unwrap(), vec!["rust", "gpui"]);
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
