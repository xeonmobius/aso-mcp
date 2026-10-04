//! Snapshot history for competitor metadata diffing.
//! Every successful lookup/details call auto-saves a row; track_diff compares
//! the two most recent snapshots per app. DB is tiny (~250 bytes/row).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

pub struct History {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub store: String,
    pub app_id: String,
    pub title: String,
    pub seller: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub description_hash: String,
    pub fetched_at: i64,
}

#[derive(Debug, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
pub struct DiffResult {
    pub store: String,
    pub app_id: String,
    pub snapshots: usize,
    /// Days between the two most recent snapshots
    pub days_between: i64,
    pub changes: Vec<FieldChange>,
    pub rating_count_delta: Option<i64>,
    /// Rating-count change per day across the interval
    pub rating_velocity_per_day: Option<f64>,
    pub status: String,
}

fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".aso-mcp");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("history.db")
}

impl History {
    pub fn open() -> Result<Self> {
        Self::open_at(default_db_path())
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path).context("opening history db")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshot (
                id INTEGER PRIMARY KEY,
                store TEXT NOT NULL,
                app_id TEXT NOT NULL,
                title TEXT NOT NULL,
                seller TEXT NOT NULL DEFAULT '',
                price TEXT,
                rating REAL,
                rating_count INTEGER,
                description_hash TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshot_app
                ON snapshot(store, app_id, fetched_at);",
        )
        .context("creating schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save(&self, snap: &Snapshot) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO snapshot (store, app_id, title, seller, price, rating, rating_count, description_hash, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                snap.store,
                snap.app_id,
                snap.title,
                snap.seller,
                snap.price,
                snap.rating,
                snap.rating_count,
                snap.description_hash,
                snap.fetched_at,
            ],
        )
        .context("inserting snapshot")?;
        Ok(())
    }

    fn two_latest(conn: &Connection, store: &str, app_id: &str) -> Result<Vec<Snapshot>> {
        let mut stmt = conn.prepare(
            "SELECT store, app_id, title, seller, price, rating, rating_count, description_hash, fetched_at
             FROM snapshot WHERE store = ?1 AND app_id = ?2
             ORDER BY fetched_at DESC, id DESC LIMIT 2",
        )?;
        let rows = stmt.query_map(rusqlite::params![store, app_id], |row| {
            Ok(Snapshot {
                store: row.get(0)?,
                app_id: row.get(1)?,
                title: row.get(2)?,
                seller: row.get(3)?,
                price: row.get(4)?,
                rating: row.get(5)?,
                rating_count: row.get(6)?,
                description_hash: row.get(7)?,
                fetched_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn diff(&self, store: &str, app_id: &str) -> Result<DiffResult> {
        let conn = self.conn.lock().unwrap();
        let snaps = Self::two_latest(&conn, store, app_id)?;
        let count = snaps.len();
        if count < 2 {
            return Ok(DiffResult {
                store: store.to_string(),
                app_id: app_id.to_string(),
                snapshots: count,
                days_between: 0,
                changes: Vec::new(),
                rating_count_delta: None,
                rating_velocity_per_day: None,
                status: "insufficient_history".into(),
            });
        }
        let (new, old) = (&snaps[0], &snaps[1]);
        let days = (new.fetched_at - old.fetched_at).max(0) / 86400;

        let mut changes = Vec::new();
        if old.title != new.title {
            changes.push(FieldChange {
                field: "title".into(),
                old: old.title.clone(),
                new: new.title.clone(),
            });
        }
        if old.seller != new.seller {
            changes.push(FieldChange {
                field: "seller".into(),
                old: old.seller.clone(),
                new: new.seller.clone(),
            });
        }
        if old.price != new.price {
            changes.push(FieldChange {
                field: "price".into(),
                old: old.price.clone().unwrap_or_default(),
                new: new.price.clone().unwrap_or_default(),
            });
        }
        let rating_delta = match (old.rating_count, new.rating_count) {
            (Some(o), Some(n)) if o != n => Some(n - o),
            _ => None,
        };
        if old.description_hash != new.description_hash {
            changes.push(FieldChange {
                field: "description".into(),
                old: "hash:".to_string() + &old.description_hash[..12],
                new: "hash:".to_string() + &new.description_hash[..12],
            });
        }
        let velocity = rating_delta.map(|d| {
            if days > 0 {
                d as f64 / days as f64
            } else {
                d as f64
            }
        });
        // Rating count always grows; only metadata changes count as "changed".
        // Velocity is informational (campaign/uninstalls signal).
        Ok(DiffResult {
            store: store.to_string(),
            app_id: app_id.to_string(),
            snapshots: count,
            days_between: days,
            status: if changes.is_empty() {
                "unchanged".into()
            } else {
                "changed".into()
            },
            changes,
            rating_count_delta: rating_delta,
            rating_velocity_per_day: velocity,
        })
    }

    pub fn tracked(&self) -> Result<Vec<(String, String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT store, app_id, COUNT(*), MAX(fetched_at)
             FROM snapshot GROUP BY store, app_id ORDER BY app_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn save_reviews(&self, store: &str, app_id: &str, reviews: &[(String, u8, String)]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS review_log (
                id INTEGER PRIMARY KEY,
                store TEXT NOT NULL,
                app_id TEXT NOT NULL,
                review_id TEXT NOT NULL,
                score INTEGER NOT NULL,
                text TEXT NOT NULL,
                fetched_at INTEGER NOT NULL,
                UNIQUE(store, app_id, review_id)
            );",
        )?;
        let now = now_secs();
        let mut saved = 0;
        for (review_id, score, text) in reviews {
            saved += conn
                .execute(
                    "INSERT OR IGNORE INTO review_log (store, app_id, review_id, score, text, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![store, app_id, review_id, score, text, now],
                )
                .context("inserting review")?;
        }
        Ok(saved)
    }

    pub fn review_stats(&self, store: &str, app_id: &str) -> Result<(i64, i64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*), MIN(fetched_at) FROM review_log WHERE store = ?1 AND app_id = ?2",
            rusqlite::params![store, app_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("review stats")
    }
}

/// Stable 64-bit hash for change detection (FNV-1a). Not cryptographic; fine here.
pub fn fnv1a(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(app_id: &str, title: &str, rating_count: i64, desc: &str, at: i64) -> Snapshot {
        Snapshot {
            store: "apple".into(),
            app_id: app_id.into(),
            title: title.into(),
            seller: "Dev".into(),
            price: Some("Free".into()),
            rating: Some(4.5),
            rating_count: Some(rating_count),
            description_hash: fnv1a(desc),
            fetched_at: at,
        }
    }

    #[test]
    fn diff_lifecycle() {
        let h = History::open_at(std::env::temp_dir().join(format!("aso-test-{}", now_secs()))).unwrap();
        // one snapshot -> insufficient
        h.save(&snap("1", "Old Title", 1000, "desc v1", 0)).unwrap();
        let d = h.diff("apple", "1").unwrap();
        assert_eq!(d.status, "insufficient_history");

        // changed title + rating growth, 7 days later
        h.save(&snap("1", "New Title", 1500, "desc v1", 7 * 86400)).unwrap();
        let d = h.diff("apple", "1").unwrap();
        assert_eq!(d.status, "changed");
        assert_eq!(d.days_between, 7);
        assert_eq!(d.rating_count_delta, Some(500));
        assert!((d.rating_velocity_per_day.unwrap() - 500.0 / 7.0).abs() < 0.01);
        assert!(d.changes.iter().any(|c| c.field == "title"));
        assert!(!d.changes.iter().any(|c| c.field == "description"));

        // unchanged week
        h.save(&snap("1", "New Title", 1600, "desc v1", 14 * 86400)).unwrap();
        let d = h.diff("apple", "1").unwrap();
        assert_eq!(d.status, "unchanged");
        assert_eq!(d.rating_count_delta, Some(100));

        let tracked = h.tracked().unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].2, 3);
    }
}
