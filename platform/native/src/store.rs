//! SQLite owns atomic source/provider replacement; failed observations hide old facts.
use craft_engine::{Extraction, SCHEMA_VERSION, valid_path};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        Self::initialize(connection)
    }

    fn initialize(connection: Connection) -> Result<Self, String> {
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err("unsupported evidence schema; rebuild with a compatible binary".into());
        }
        connection
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        if version == 0 {
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count != 0 {
                return Err("unversioned nonempty evidence database".into());
            }
            connection.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                INSERT INTO metadata VALUES ('generation',0),('inventory_complete',0),('inventory_epoch',0);
                CREATE TABLE sources (path TEXT NOT NULL, provider TEXT NOT NULL, hash TEXT NOT NULL,
                    outcome TEXT NOT NULL, diagnostic TEXT, observed_at TEXT NOT NULL, epoch INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(path,provider));
                CREATE TABLE evidence (id TEXT PRIMARY KEY, path TEXT NOT NULL, provider TEXT NOT NULL,
                    kind TEXT NOT NULL, name TEXT NOT NULL, target TEXT, record TEXT NOT NULL,
                    FOREIGN KEY(path,provider) REFERENCES sources(path,provider) ON DELETE CASCADE);
                CREATE INDEX evidence_name ON evidence(name,id);
                CREATE INDEX evidence_source ON evidence(path,provider,id);
                CREATE INDEX evidence_target ON evidence(target,id);
                PRAGMA user_version=1;
                COMMIT;").map_err(|e| e.to_string())?;
        }
        Ok(Self { connection })
    }

    pub fn replace(
        &mut self,
        extraction: &Extraction,
        observed_at: &str,
        epoch: i64,
    ) -> Result<(), String> {
        extraction.validate()?;
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO sources(path,provider,hash,outcome,observed_at,epoch) VALUES (?1,?2,?3,'ok',?4,?5)
            ON CONFLICT(path,provider) DO UPDATE SET hash=excluded.hash,outcome='ok',diagnostic=NULL,observed_at=excluded.observed_at,epoch=excluded.epoch",
            params![extraction.path, extraction.provider, extraction.hash, observed_at, epoch]).map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM evidence WHERE path=?1 AND provider=?2",
            params![extraction.path, extraction.provider],
        )
        .map_err(|e| e.to_string())?;
        for item in &extraction.evidence {
            tx.execute(
                "INSERT INTO evidence VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    item.id,
                    extraction.path,
                    extraction.provider,
                    item.kind,
                    item.name,
                    item.target,
                    serde_json::to_string(item).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn fail(
        &mut self,
        path: &str,
        provider: &str,
        hash: &str,
        diagnostic: &str,
        observed_at: &str,
        epoch: i64,
    ) -> Result<(), String> {
        if !valid_path(path) {
            return Err("invalid source path".into());
        }
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        // Failed content is cached so repeated bounded refreshes can advance.
        // Retire old facts rather than bind them to the failed content hash.
        tx.execute("INSERT INTO sources VALUES (?1,?2,?3,'failed',?4,?5,?6)
            ON CONFLICT(path,provider) DO UPDATE SET hash=excluded.hash,outcome='failed',diagnostic=excluded.diagnostic,observed_at=excluded.observed_at,epoch=excluded.epoch",
            params![path,provider,hash,diagnostic,observed_at,epoch]).map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM evidence WHERE path=?1 AND provider=?2",
            params![path, provider],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn defer(&mut self, path: &str, provider: &str, epoch: i64) -> Result<(), String> {
        if !valid_path(path) {
            return Err("invalid source path".into());
        }
        self.connection.execute("INSERT INTO sources(path,provider,hash,outcome,observed_at,epoch) VALUES (?1,?2,'','pending','',?3)
            ON CONFLICT(path,provider) DO UPDATE SET outcome='pending',diagnostic=NULL,epoch=excluded.epoch",
            params![path,provider,epoch]).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Retire contributions before acquisition, including after provider upgrades.
    pub fn retain_providers(&mut self, active: &[&str]) -> Result<usize, String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        let providers = {
            let mut statement = tx
                .prepare("SELECT DISTINCT provider FROM sources")
                .map_err(|e| e.to_string())?;
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let mut removed = 0;
        for provider in providers {
            if !active.contains(&provider.as_str()) {
                removed += tx
                    .execute("DELETE FROM sources WHERE provider=?1", [&provider])
                    .map_err(|e| e.to_string())?;
            }
        }
        if removed > 0 {
            tx.execute(
                "UPDATE metadata SET value=value+1 WHERE key='generation'",
                [],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
                [],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(removed)
    }

    pub fn begin_inventory(&mut self) -> Result<i64, String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key IN ('inventory_epoch','generation')",
            [],
        )
        .map_err(|e| e.to_string())?;
        let epoch = tx
            .query_row(
                "SELECT value FROM metadata WHERE key='inventory_epoch'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(epoch)
    }

    pub fn reuse(
        &self,
        path: &str,
        provider: &str,
        hash: &str,
        epoch: i64,
    ) -> Result<bool, String> {
        let count = self.connection.execute("UPDATE sources SET epoch=?4 WHERE path=?1 AND provider=?2 AND hash=?3 AND outcome IN ('ok','failed')",
            params![path,provider,hash,epoch]).map_err(|e| e.to_string())?;
        Ok(count != 0)
    }

    pub fn finish_inventory(&mut self, epoch: i64) -> Result<usize, String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        let removed = tx
            .execute("DELETE FROM sources WHERE epoch<>?1", params![epoch])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=1 WHERE key='inventory_complete'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(removed)
    }

    pub fn backup(&self, destination: &Path) -> Result<(), String> {
        if destination.try_exists().map_err(|e| e.to_string())? {
            return Err(
                "backup already exists; preserve or remove it before another backup".into(),
            );
        }
        self.connection
            .execute(
                "VACUUM INTO ?1",
                [destination.to_str().ok_or("non-UTF-8 backup path")?],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Rebuild only derived evidence, preserving the store identity and generation history.
    pub fn clear(&mut self) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM sources", [])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn status(&self) -> Result<Value, String> {
        let scalar = |sql| {
            self.connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .map_err(|e| e.to_string())
        };
        Ok(json!({"schemaVersion":SCHEMA_VERSION,
            "generation":scalar("SELECT value FROM metadata WHERE key='generation'")?,
            "inventoryComplete":scalar("SELECT value FROM metadata WHERE key='inventory_complete'")? == 1,
            "sources":scalar("SELECT count(*) FROM sources")?,
            "failedSources":scalar("SELECT count(*) FROM sources WHERE outcome='failed'")?,
            "pendingSources":scalar("SELECT count(*) FROM sources WHERE outcome='pending'")?,
            "evidence":scalar("SELECT count(*) FROM evidence e JOIN sources s USING(path,provider) WHERE s.outcome='ok'")?,
            "freshness":"stored-snapshot", "completeness":"unknown outside declared provider coverage"}))
    }

    pub fn query(
        &self,
        field: &str,
        value: &str,
        after: &str,
        limit: usize,
    ) -> Result<Value, String> {
        let column = match field {
            "id" => "e.id",
            "name" => "e.name",
            "path" => "e.path",
            "target" => "e.target",
            _ => return Err("query field must be id, name, path or target".into()),
        };
        if !(1..=200).contains(&limit) || value.len() > 4096 || after.len() > 64 {
            return Err("query exceeds bounds".into());
        }
        let sql = format!("SELECT e.id,e.record,s.hash,s.observed_at,e.path,e.provider FROM evidence e JOIN sources s USING(path,provider)
            WHERE {column}=?1 AND e.id>?2 AND s.outcome='ok' ORDER BY e.id LIMIT ?3");
        let mut statement = self.connection.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(params![value, after, (limit + 1) as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut results = Vec::new();
        for row in rows {
            let (id, record, hash, observed_at, path, provider) = row.map_err(|e| e.to_string())?;
            results.push(json!({"id":id,"evidence":serde_json::from_str::<Value>(&record).map_err(|e|e.to_string())?,"path":path,"hash":hash,"provider":provider,"observedAt":observed_at}));
        }
        let more = results.len() > limit;
        results.truncate(limit);
        let next = if more {
            results.last().and_then(|v| v.get("id")).cloned()
        } else {
            None
        };
        Ok(json!({"results":results,"continuation":next,"coverage":self.status()?}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::javascript::{PROVIDER, extract};
    #[test]
    fn refused_write_preserves_previous_generation_and_facts() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let epoch = store.begin_inventory().unwrap();
        store
            .replace(&extract("a.js", b"const before = 1;").unwrap(), "1", epoch)
            .unwrap();
        let generation = store.status().unwrap()["generation"].clone();
        store
            .connection
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        assert!(
            store
                .replace(&extract("a.js", b"const after = 1;").unwrap(), "2", epoch)
                .is_err()
        );
        assert_eq!(store.status().unwrap()["generation"], generation);
        assert_eq!(
            store.query("name", "before", "", 10).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.query("name", "after", "", 10).unwrap()["results"],
            json!([])
        );
    }

    #[test]
    fn retired_provider_cannot_leave_queryable_facts() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let epoch = store.begin_inventory().unwrap();
        store
            .replace(
                &extract("a.js", b"const previous = 1;").unwrap(),
                "1",
                epoch,
            )
            .unwrap();
        store.finish_inventory(epoch).unwrap();
        assert_eq!(store.retain_providers(&[PROVIDER]).unwrap(), 0);
        assert_eq!(
            store.retain_providers(&["replacement-provider"]).unwrap(),
            1
        );
        assert_eq!(store.status().unwrap()["evidence"], 0);
        assert_eq!(store.status().unwrap()["inventoryComplete"], false);
        assert_eq!(
            store.query("name", "previous", "", 10).unwrap()["results"],
            json!([])
        );
        assert_eq!(store.retain_providers(&[]).unwrap(), 0);
    }

    #[test]
    fn interrupted_inventory_preserves_committed_sources_for_restart() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let epoch = store.begin_inventory().unwrap();
        let extraction = extract("a.js", b"const retained = 1;").unwrap();
        store.replace(&extraction, "1", epoch).unwrap();
        assert_eq!(store.status().unwrap()["inventoryComplete"], false);
        let restarted = store.begin_inventory().unwrap();
        assert!(
            store
                .reuse("a.js", PROVIDER, &extraction.hash, restarted)
                .unwrap()
        );
        assert_eq!(store.finish_inventory(restarted).unwrap(), 0);
        assert_eq!(store.status().unwrap()["evidence"], 1);
    }

    #[test]
    fn replacement_failure_empty_success_and_deletion_are_distinct() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let epoch = store.begin_inventory().unwrap();
        store
            .replace(&extract("a.js", b"const before = 1;").unwrap(), "1", epoch)
            .unwrap();
        assert_eq!(
            store.query("name", "before", "", 10).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        store
            .fail("a.js", PROVIDER, "", "syntax error", "2", epoch)
            .unwrap();
        assert!(
            store.query("name", "before", "", 10).unwrap()["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.status().unwrap()["failedSources"], 1);
        store
            .replace(&extract("a.js", b"// empty").unwrap(), "3", epoch)
            .unwrap();
        assert_eq!(store.status().unwrap()["evidence"], 0);
        store.finish_inventory(epoch).unwrap();
        let next = store.begin_inventory().unwrap();
        assert_eq!(
            store.status().unwrap()["sources"],
            1,
            "partial inventory cannot retire unseen sources"
        );
        assert_eq!(store.finish_inventory(next).unwrap(), 1);
        assert_eq!(store.status().unwrap()["sources"], 0);
    }
    #[test]
    fn unknown_schema_refuses_writes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(Store::initialize(connection).is_err());
    }
}
