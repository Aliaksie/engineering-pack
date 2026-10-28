//! SQLite owns atomic source/provider replacement; failed observations hide old facts.
use crate::Error;
use craft_engine::{Evidence, Extraction, SCHEMA_VERSION, valid_path, values::lower_hex};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Refresh writes share one transaction per batch so a large refresh syncs per batch, not
/// per source, and a 64 MiB page cache keeps random-ID index inserts from rereading pages.
/// Closing the store with a batch open rolls back only that batch.
const BATCH_SOURCES: usize = 512;
const BATCH_TIME: Duration = Duration::from_secs(1);

pub struct Store {
    connection: Connection,
    batch: Option<Batch>,
}

struct Batch {
    started: Instant,
    sources: usize,
}

impl Store {
    /// Open or create the store; a new file is private, and SQLite gives its journal the
    /// same mode. Evidence is derived, so a store from another schema version is discarded
    /// and the next refresh rebuilds it; an unreadable file still fails.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut connection = Self::connect(path)?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            drop(connection);
            for file in [path.to_path_buf(), path.with_extension("sqlite-journal")] {
                match std::fs::remove_file(&file) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.into());
                    }
                    _ => {}
                }
            }
            connection = Self::connect(path)?;
        }
        Self::initialize(connection)
    }

    fn connect(path: &Path) -> Result<Connection, Error> {
        match crate::files::create_private_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(error.into());
            }
            _ => {}
        }
        Ok(Connection::open(path)?)
    }

    fn initialize(connection: Connection) -> Result<Self, Error> {
        connection.busy_timeout(Duration::from_secs(2))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err("unsupported evidence schema; rebuild with a compatible binary".into());
        }
        connection.execute_batch("PRAGMA foreign_keys=ON;")?;
        if version == 0 {
            let count: i64 = connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if count != 0 {
                return Err("unversioned nonempty evidence database".into());
            }
            connection.execute_batch(&format!("BEGIN IMMEDIATE;
                -- `exclusions` is the JSON prefix list the last complete inventory applied.
                CREATE TABLE metadata (key TEXT PRIMARY KEY, value NOT NULL);
                INSERT INTO metadata VALUES ('generation',0),('inventory_complete',0),('inventory_epoch',0),
                    ('exclusions','[]'),('excluded_sources',0);
                CREATE TABLE sources (sid INTEGER PRIMARY KEY, path TEXT NOT NULL, provider TEXT NOT NULL,
                    hash TEXT NOT NULL, outcome TEXT NOT NULL, diagnostic TEXT, observed_at TEXT NOT NULL,
                    epoch INTEGER NOT NULL DEFAULT 0, records INTEGER NOT NULL DEFAULT 0, UNIQUE(path,provider));
                -- IDs are SHA-256 digests stored as 32-byte blobs; byte order equals hex order.
                CREATE TABLE evidence (id BLOB PRIMARY KEY,
                    sid INTEGER NOT NULL REFERENCES sources(sid) ON DELETE CASCADE,
                    kind TEXT NOT NULL, name TEXT NOT NULL, target TEXT, line INTEGER NOT NULL,
                    col INTEGER NOT NULL, start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL,
                    container BLOB) WITHOUT ROWID;
                CREATE INDEX evidence_name ON evidence(name);
                CREATE INDEX evidence_source ON evidence(sid);
                CREATE INDEX evidence_target ON evidence(target);
                PRAGMA user_version={SCHEMA_VERSION};
                COMMIT;"))?;
        }
        Ok(Self {
            connection,
            batch: None,
        })
    }

    pub fn replace(
        &mut self,
        extraction: &Extraction,
        observed_at: &str,
        epoch: i64,
    ) -> Result<(), Error> {
        extraction.validate()?;
        let tx = self.connection.savepoint()?;
        let sid: i64 = tx.query_row("INSERT INTO sources(path,provider,hash,outcome,observed_at,epoch,records) VALUES (?1,?2,?3,'ok',?4,?5,?6)
            ON CONFLICT(path,provider) DO UPDATE SET hash=excluded.hash,outcome='ok',diagnostic=NULL,observed_at=excluded.observed_at,epoch=excluded.epoch,records=excluded.records
            RETURNING sid",
            params![extraction.path, extraction.provider, extraction.hash, observed_at, epoch, extraction.evidence.len() as i64], |r| r.get(0))?;
        tx.execute("DELETE FROM evidence WHERE sid=?1", [sid])?;
        let mut insert = tx.prepare_cached(
            "INSERT INTO evidence VALUES (unhex(?1),?2,?3,?4,?5,?6,?7,?8,?9,unhex(?10))",
        )?;
        for item in &extraction.evidence {
            insert.execute(params![
                item.id,
                sid,
                item.kind,
                item.name,
                item.target,
                item.line as i64,
                item.column as i64,
                item.start_byte as i64,
                item.end_byte as i64,
                item.container
            ])?;
        }
        drop(insert);
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )?;
        tx.commit()?;
        self.written()
    }

    pub fn fail(
        &mut self,
        path: &str,
        provider: &str,
        hash: &str,
        diagnostic: &str,
        observed_at: &str,
        epoch: i64,
    ) -> Result<(), Error> {
        if !valid_path(path) {
            return Err("invalid source path".into());
        }
        let tx = self.connection.savepoint()?;
        // Failed content is cached so repeated bounded refreshes can advance.
        // Retire old facts rather than bind them to the failed content hash.
        let sid: i64 = tx.query_row("INSERT INTO sources(path,provider,hash,outcome,diagnostic,observed_at,epoch) VALUES (?1,?2,?3,'failed',?4,?5,?6)
            ON CONFLICT(path,provider) DO UPDATE SET hash=excluded.hash,outcome='failed',diagnostic=excluded.diagnostic,observed_at=excluded.observed_at,epoch=excluded.epoch,records=0
            RETURNING sid",
            params![path,provider,hash,diagnostic,observed_at,epoch], |r| r.get(0))?;
        tx.execute("DELETE FROM evidence WHERE sid=?1", [sid])?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )?;
        tx.commit()?;
        self.written()
    }

    pub fn defer(&mut self, path: &str, provider: &str, epoch: i64) -> Result<(), Error> {
        if !valid_path(path) {
            return Err("invalid source path".into());
        }
        self.connection.execute("INSERT INTO sources(path,provider,hash,outcome,observed_at,epoch) VALUES (?1,?2,'','pending','',?3)
            ON CONFLICT(path,provider) DO UPDATE SET outcome='pending',diagnostic=NULL,epoch=excluded.epoch",
            params![path,provider,epoch])?;
        self.written()
    }

    /// Retire contributions before acquisition, including after provider upgrades.
    pub fn retain_providers(&mut self, active: &[&str]) -> Result<usize, Error> {
        let tx = self.connection.transaction()?;
        let providers = {
            let mut statement = tx.prepare("SELECT DISTINCT provider FROM sources")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut removed = 0;
        for provider in providers {
            if !active.contains(&provider.as_str()) {
                removed += tx.execute("DELETE FROM sources WHERE provider=?1", [&provider])?;
            }
        }
        if removed > 0 {
            tx.execute(
                "UPDATE metadata SET value=value+1 WHERE key='generation'",
                [],
            )?;
            tx.execute(
                "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
                [],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn begin_inventory(&mut self) -> Result<i64, Error> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
            [],
        )?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key IN ('inventory_epoch','generation')",
            [],
        )?;
        let epoch = tx.query_row(
            "SELECT value FROM metadata WHERE key='inventory_epoch'",
            [],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(epoch)
    }

    pub fn reuse(
        &mut self,
        path: &str,
        provider: &str,
        hash: &str,
        epoch: i64,
    ) -> Result<bool, Error> {
        let count = self.connection.prepare_cached("UPDATE sources SET epoch=?4 WHERE path=?1 AND provider=?2 AND hash=?3 AND outcome IN ('ok','failed')")?
            .execute(params![path,provider,hash,epoch])?;
        self.written()?;
        Ok(count != 0)
    }

    pub fn begin_batch(&mut self) -> Result<(), Error> {
        self.connection
            .execute_batch("PRAGMA cache_size=-65536; BEGIN IMMEDIATE")?;
        self.batch = Some(Batch {
            started: Instant::now(),
            sources: 0,
        });
        Ok(())
    }

    pub fn commit_batch(&mut self) -> Result<(), Error> {
        if self.batch.take().is_some() {
            self.connection.execute_batch("COMMIT")?;
        }
        Ok(())
    }

    fn written(&mut self) -> Result<(), Error> {
        let Some(batch) = &mut self.batch else {
            return Ok(());
        };
        batch.sources += 1;
        if batch.sources >= BATCH_SOURCES || batch.started.elapsed() >= BATCH_TIME {
            self.commit_batch()?;
            self.begin_batch()?;
        }
        Ok(())
    }

    /// Retire sources this inventory did not see, including newly excluded ones, and record
    /// the exclusions it applied with the number of eligible sources they left out.
    pub fn finish_inventory(
        &mut self,
        epoch: i64,
        exclusions: &[String],
        excluded: u64,
    ) -> Result<usize, Error> {
        let tx = self.connection.transaction()?;
        let removed = tx.execute("DELETE FROM sources WHERE epoch<>?1", params![epoch])?;
        tx.execute(
            "UPDATE metadata SET value=?1 WHERE key='exclusions'",
            [serde_json::to_string(exclusions).expect("strings serialize")],
        )?;
        tx.execute(
            "UPDATE metadata SET value=?1 WHERE key='excluded_sources'",
            [excluded as i64],
        )?;
        tx.execute(
            "UPDATE metadata SET value=1 WHERE key='inventory_complete'",
            [],
        )?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Snapshot into a new private file; SQLite accepts an empty target for `VACUUM INTO`.
    pub fn backup(&self, destination: &Path) -> Result<(), Error> {
        let name = destination.to_str().ok_or("non-UTF-8 backup path")?;
        match crate::files::create_private_file(destination) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(
                    "backup already exists; preserve or remove it before another backup".into(),
                );
            }
            created => drop(created?),
        }
        let result = self.connection.execute("VACUUM INTO ?1", [name]);
        if result.is_err() {
            let _ = std::fs::remove_file(destination);
        }
        result.map(drop).map_err(Error::from)
    }

    /// Rebuild only derived evidence, preserving the store identity and generation history.
    pub fn clear(&mut self) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM sources", [])?;
        tx.execute(
            "UPDATE metadata SET value=0 WHERE key='inventory_complete'",
            [],
        )?;
        tx.execute(
            "UPDATE metadata SET value=value+1 WHERE key='generation'",
            [],
        )?;
        tx.commit().map_err(Error::from)
    }

    /// Counts come from the sources table; `records` is written with each source's evidence.
    pub fn status(&self) -> Result<Value, Error> {
        let scalar = |sql| {
            self.connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .map_err(Error::from)
        };
        let (sources, failed, pending, evidence) = self.connection.query_row(
            "SELECT count(*), count(*) FILTER (WHERE outcome='failed'), count(*) FILTER (WHERE outcome='pending'),
                coalesce(sum(records) FILTER (WHERE outcome='ok'),0) FROM sources",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
        )?;
        let exclusions: String = self.connection.query_row(
            "SELECT value FROM metadata WHERE key='exclusions'",
            [],
            |r| r.get(0),
        )?;
        let exclusions: Value = serde_json::from_str(&exclusions)
            .map_err(|_| Error::from("invalid stored evidence exclusions"))?;
        Ok(json!({"schemaVersion":SCHEMA_VERSION,
            "exclusions":exclusions,
            "excludedSources":scalar("SELECT value FROM metadata WHERE key='excluded_sources'")?,
            "generation":scalar("SELECT value FROM metadata WHERE key='generation'")?,
            "inventoryComplete":scalar("SELECT value FROM metadata WHERE key='inventory_complete'")? == 1,
            "sources":sources, "failedSources":failed, "pendingSources":pending, "evidence":evidence,
            "freshness":"stored-snapshot", "completeness":"unknown outside declared provider coverage"}))
    }

    pub fn query(
        &self,
        field: &str,
        value: &str,
        after: &str,
        limit: usize,
        source_path: Option<&str>,
    ) -> Result<Value, Error> {
        let column = match field {
            "id" => "e.id=unhex(?1)",
            "name" => "e.name=?1",
            "path" => "s.path=?1",
            "target" => "e.target=?1",
            _ => return Err("query field must be id, name, path or target".into()),
        };
        if !(1..=200).contains(&limit) || value.len() > 4096 {
            return Err("query exceeds bounds".into());
        }
        if !after.is_empty() && !lower_hex(after, 64) {
            return Err("continuation must be an evidence id".into());
        }
        if source_path.is_some_and(|path| !valid_path(path)) {
            return Err("sourcePath must be a relative source path".into());
        }
        let source_filter = if source_path.is_some() {
            "AND s.path=?4"
        } else {
            "AND ?4 IS NULL"
        };
        let sql = format!("SELECT lower(hex(e.id)),e.kind,e.name,e.line,e.col,e.start_byte,e.end_byte,e.target,
                CASE WHEN e.container IS NULL THEN NULL ELSE lower(hex(e.container)) END,
                s.hash,s.observed_at,s.path,s.provider FROM evidence e JOIN sources s USING(sid)
            WHERE {column} AND e.id>unhex(?2) AND s.outcome='ok' {source_filter} ORDER BY e.id LIMIT ?3");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![value, after, (limit + 1) as i64, source_path],
            |r| {
                let evidence = Evidence {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    name: r.get(2)?,
                    line: r.get::<_, i64>(3)? as usize,
                    column: r.get::<_, i64>(4)? as usize,
                    start_byte: r.get::<_, i64>(5)? as usize,
                    end_byte: r.get::<_, i64>(6)? as usize,
                    target: r.get(7)?,
                    container: r.get(8)?,
                };
                Ok(json!({"id":evidence.id,"evidence":evidence,"path":r.get::<_, String>(11)?,
                    "hash":r.get::<_, String>(9)?,"provider":r.get::<_, String>(12)?,"observedAt":r.get::<_, String>(10)?}))
            },
        )?;
        let mut results = rows.collect::<Result<Vec<_>, _>>()?;
        let more = results.len() > limit;
        results.truncate(limit);
        let next = if more {
            results.last().and_then(|v| v.get("id")).cloned()
        } else {
            None
        };
        Ok(
            json!({"results":results,"continuation":next,"sourcePath":source_path,"coverage":self.status()?}),
        )
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
            store.query("name", "before", "", 10, None).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.query("name", "after", "", 10, None).unwrap()["results"],
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
        store.finish_inventory(epoch, &[], 0).unwrap();
        assert_eq!(store.retain_providers(&[PROVIDER]).unwrap(), 0);
        assert_eq!(
            store.retain_providers(&["replacement-provider"]).unwrap(),
            1
        );
        assert_eq!(store.status().unwrap()["evidence"], 0);
        assert_eq!(store.status().unwrap()["inventoryComplete"], false);
        assert_eq!(
            store.query("name", "previous", "", 10, None).unwrap()["results"],
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
        assert_eq!(store.finish_inventory(restarted, &[], 0).unwrap(), 0);
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
            store.query("name", "before", "", 10, None).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        store
            .fail("a.js", PROVIDER, "", "syntax error", "2", epoch)
            .unwrap();
        assert!(
            store.query("name", "before", "", 10, None).unwrap()["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.status().unwrap()["failedSources"], 1);
        store
            .replace(&extract("a.js", b"// empty").unwrap(), "3", epoch)
            .unwrap();
        assert_eq!(store.status().unwrap()["evidence"], 0);
        store.finish_inventory(epoch, &[], 0).unwrap();
        let next = store.begin_inventory().unwrap();
        assert_eq!(
            store.status().unwrap()["sources"],
            1,
            "partial inventory cannot retire unseen sources"
        );
        assert_eq!(store.finish_inventory(next, &[], 0).unwrap(), 1);
        assert_eq!(store.status().unwrap()["sources"], 0);
    }
    #[test]
    fn interrupted_batch_keeps_committed_batches_only() {
        let database =
            std::env::temp_dir().join(format!("craft-store-batch-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&database);
        let mut store = Store::open(&database).unwrap();
        let epoch = store.begin_inventory().unwrap();
        store.begin_batch().unwrap();
        for index in 0..=BATCH_SOURCES {
            let source = format!("const v{index} = 1;");
            store
                .replace(
                    &extract(&format!("s{index}.js"), source.as_bytes()).unwrap(),
                    "1",
                    epoch,
                )
                .unwrap();
        }
        drop(store);
        let store = Store::open(&database).unwrap();
        let committed = store.status().unwrap()["sources"].as_u64().unwrap() as usize;
        drop(store);
        std::fs::remove_file(&database).unwrap();
        assert!(
            committed > 0 && committed <= BATCH_SOURCES,
            "only full batches survive: {committed}"
        );
    }

    #[test]
    fn other_schema_store_is_discarded_and_counts_follow_outcomes() {
        let database =
            std::env::temp_dir().join(format!("craft-store-schema-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&database);
        let earlier = Connection::open(&database).unwrap();
        earlier
            .execute_batch("CREATE TABLE evidence (id TEXT); PRAGMA user_version=1;")
            .unwrap();
        drop(earlier);
        let mut store = Store::open(&database).unwrap();
        assert_eq!(store.status().unwrap()["sources"], 0);
        let epoch = store.begin_inventory().unwrap();
        store
            .replace(
                &extract("a.js", b"const a = 1, b = 2;").unwrap(),
                "1",
                epoch,
            )
            .unwrap();
        store
            .replace(&extract("c.js", b"const c = 1;").unwrap(), "1", epoch)
            .unwrap();
        store.defer("c.js", PROVIDER, epoch).unwrap();
        let status = store.status().unwrap();
        assert_eq!(
            (
                &status["sources"],
                &status["pendingSources"],
                &status["evidence"]
            ),
            (&json!(2), &json!(1), &json!(2))
        );
        let page = store.query("name", "a", "", 1, None).unwrap();
        assert_eq!(page["results"][0]["evidence"]["name"], "a");
        assert!(store.query("name", "a", "not-an-id", 1, None).is_err());
        drop(store);
        std::fs::remove_file(&database).unwrap();
    }

    #[test]
    fn unknown_schema_refuses_writes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(Store::initialize(connection).is_err());
    }
}
