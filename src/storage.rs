//! Embedded SQLite storage engine for MobHub faction records, wars, and payout registers.

use rusqlite::{params, Connection, Result};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredWar {
    pub id: i64,
    pub name: String,
    pub cache_value: i64,
    pub status: String,
    pub data_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredBalance {
    pub torn_id: i64,
    pub name: String,
    pub balance: i64,
    pub updated_at: String,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_tables()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_tables()?;
        Ok(db)
    }

    pub fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS wars (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                cache_value INTEGER NOT NULL,
                status TEXT NOT NULL,
                data_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS payout_registers (
                war_id INTEGER PRIMARY KEY,
                policy_json TEXT NOT NULL,
                result_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS faction_balances (
                torn_id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                balance INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS item_cache (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                market_price INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    pub fn save_war(
        &self,
        id: i64,
        name: &str,
        cache_value: i64,
        status: &str,
        data_json: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO wars (id, name, cache_value, status, data_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                cache_value = excluded.cache_value,
                status = excluded.status,
                data_json = excluded.data_json",
            params![id, name, cache_value, status, data_json, now],
        )?;
        Ok(())
    }

    pub fn get_war(&self, id: i64) -> Result<Option<StoredWar>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, cache_value, status, data_json, created_at FROM wars WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(StoredWar {
                id: row.get(0)?,
                name: row.get(1)?,
                cache_value: row.get(2)?,
                status: row.get(3)?,
                data_json: row.get(4)?,
                created_at: row.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_wars(&self) -> Result<Vec<StoredWar>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, cache_value, status, data_json, created_at FROM wars ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredWar {
                id: row.get(0)?,
                name: row.get(1)?,
                cache_value: row.get(2)?,
                status: row.get(3)?,
                data_json: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_payout_register(
        &self,
        war_id: i64,
        policy_json: &str,
        result_json: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO payout_registers (war_id, policy_json, result_json, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(war_id) DO UPDATE SET
                policy_json = excluded.policy_json,
                result_json = excluded.result_json",
            params![war_id, policy_json, result_json, now],
        )?;
        Ok(())
    }

    pub fn get_payout_register(&self, war_id: i64) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT policy_json, result_json FROM payout_registers WHERE war_id = ?1",
        )?;
        let mut rows = stmt.query(params![war_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_balance(&self, torn_id: i64, name: &str, balance: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO faction_balances (torn_id, name, balance, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(torn_id) DO UPDATE SET
                name = excluded.name,
                balance = excluded.balance,
                updated_at = excluded.updated_at",
            params![torn_id, name, balance, now],
        )?;
        Ok(())
    }

    pub fn list_balances(&self) -> Result<Vec<StoredBalance>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT torn_id, name, balance, updated_at FROM faction_balances ORDER BY balance DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredBalance {
                torn_id: row.get(0)?,
                name: row.get(1)?,
                balance: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_lifecycle() {
        let db = Database::open_in_memory().unwrap();
        db.save_war(40711, "GanjaWarriors", 349499999, "finished", "{}")
            .unwrap();

        let war = db.get_war(40711).unwrap().unwrap();
        assert_eq!(war.id, 40711);
        assert_eq!(war.name, "GanjaWarriors");
        assert_eq!(war.cache_value, 349499999);

        let list = db.list_wars().unwrap();
        assert_eq!(list.len(), 1);

        db.upsert_balance(2532285, "MrRazorlove", 50000000).unwrap();
        let bals = db.list_balances().unwrap();
        assert_eq!(bals.len(), 1);
        assert_eq!(bals[0].balance, 50000000);
    }
}
