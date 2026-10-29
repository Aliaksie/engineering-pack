//! SQLite owns atomic source/provider replacement; failed observations hide old facts.
use crate::Error;
use craft_engine::{Extraction, SCHEMA_VERSION, valid_path, values::lower_hex};
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
                -- `exclusions` is the JSON prefix list the last complete inventory applied and
                -- `inventory` the JSON description of how it found sources.
                CREATE TABLE metadata (key TEXT PRIMARY KEY, value NOT NULL);
                INSERT INTO metadata VALUES ('generation',0),('inventory_complete',0),('inventory_epoch',0),
                    ('exclusions','[]'),('excluded_sources',0),('inventory','null');
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
    /// how it found sources and the exclusions it applied with the number of eligible
    /// sources they left out.
    pub fn finish_inventory(
        &mut self,
        epoch: i64,
        inventory: &Value,
        exclusions: &[String],
        excluded: u64,
    ) -> Result<usize, Error> {
        let tx = self.connection.transaction()?;
        let removed = tx.execute("DELETE FROM sources WHERE epoch<>?1", params![epoch])?;
        tx.execute(
            "UPDATE metadata SET value=?1 WHERE key='inventory'",
            [inventory.to_string()],
        )?;
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
        let inventory: String = self.connection.query_row(
            "SELECT value FROM metadata WHERE key='inventory'",
            [],
            |r| r.get(0),
        )?;
        let inventory: Value = serde_json::from_str(&inventory)
            .map_err(|_| Error::from("invalid stored evidence inventory"))?;
        Ok(json!({"schemaVersion":SCHEMA_VERSION,
            "inventory":inventory,
            "exclusions":exclusions,
            "excludedSources":scalar("SELECT value FROM metadata WHERE key='excluded_sources'")?,
            "generation":scalar("SELECT value FROM metadata WHERE key='generation'")?,
            "inventoryComplete":scalar("SELECT value FROM metadata WHERE key='inventory_complete'")? == 1,
            "sources":sources, "failedSources":failed, "pendingSources":pending, "evidence":evidence,
            "freshness":"stored-snapshot", "completeness":"unknown outside declared provider coverage"}))
    }

    /// Sources without current evidence, as `(path, provider, outcome)` in path order, where
    /// the outcome is `pending` or `failed`.
    pub fn unextracted(&self) -> Result<Vec<(String, String, String)>, Error> {
        let mut statement = self.connection.prepare(
            "SELECT path,provider,outcome FROM sources WHERE outcome<>'ok' ORDER BY path,provider",
        )?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Matches in path, line, column and ID order. The continuation is the last returned ID;
    /// it resumes after that row's position, and refuses once the row is no longer stored.
    /// Source provenance is listed once per path; a truncated page reports match counts.
    pub fn query(&self, query: &Query) -> Result<Value, Error> {
        let column = match query.field {
            "id" => "e.id=unhex(?1)",
            "name" => "e.name=?1",
            "path" => "s.path=?1",
            "target" => "e.target=?1",
            _ => return Err("query field must be id, name, path or target".into()),
        };
        if !(1..=200).contains(&query.limit)
            || query.value.len() > 4096
            || query.kinds.len() > 16
            || query.under.len() > 32
            || query
                .kinds
                .iter()
                .any(|kind| kind.is_empty() || kind.len() > 128)
        {
            return Err("query exceeds bounds".into());
        }
        if !query.after.is_empty() && !lower_hex(query.after, 64) {
            return Err("continuation must be an evidence id".into());
        }
        if query.source.is_some_and(|path| !valid_path(path)) {
            return Err("sourcePath must be a relative source path".into());
        }
        let under: Vec<_> = query
            .under
            .iter()
            .map(|path| path.strip_suffix('/').unwrap_or(path))
            .collect();
        if under.iter().any(|path| !valid_path(path)) {
            return Err("under must be a relative path".into());
        }
        // Kind and directory lists bind as JSON arrays; NULL leaves the filter out.
        let list = |items: &[&str]| (!items.is_empty()).then(|| json!(items).to_string());
        let (kinds, directories) = (list(query.kinds), list(&under));
        let filters = format!(
            "{column} AND s.outcome='ok' AND (?2 IS NULL OR s.path=?2)
            AND (?3 IS NULL OR e.kind IN (SELECT value FROM json_each(?3)))
            AND (?4 IS NULL OR EXISTS (SELECT 1 FROM json_each(?4) u
                WHERE s.path=u.value OR substr(s.path,1,length(u.value)+1)=u.value||'/'))"
        );
        let filter = params![query.value, query.source, kinds, directories];
        let position = if query.after.is_empty() {
            "1"
        } else {
            let stored: bool = self.connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM evidence WHERE id=unhex(?1))",
                [query.after],
                |r| r.get(0),
            )?;
            if !stored {
                return Err("continuation is no longer stored; restart the query".into());
            }
            "(s.path,e.line,e.col,e.id)>(SELECT ps.path,p.line,p.col,p.id FROM evidence p
                JOIN sources ps USING(sid) WHERE p.id=unhex(?5))"
        };
        let sql = format!(
            "SELECT lower(hex(e.id)),e.kind,e.name,e.line,e.col,e.start_byte,e.end_byte,e.target,
                s.path,s.hash,s.provider,s.observed_at,CASE WHEN c.id IS NULL THEN NULL ELSE lower(hex(c.id)) END,c.kind,c.name,c.line
            FROM evidence e JOIN sources s USING(sid) LEFT JOIN evidence c ON c.id=e.container
            WHERE {filters} AND {position} ORDER BY s.path,e.line,e.col,e.id LIMIT ?6"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut sources = serde_json::Map::new();
        let mut results = Vec::new();
        let mut rows = statement.query(params![
            query.value,
            query.source,
            kinds,
            directories,
            query.after,
            (query.limit + 1) as i64
        ])?;
        while let Some(r) = rows.next()? {
            let path: String = r.get(8)?;
            if !sources.contains_key(&path) {
                sources.insert(
                    path.clone(),
                    json!({"hash":r.get::<_, String>(9)?,"provider":r.get::<_, String>(10)?,
                        "observedAt":r.get::<_, String>(11)?}),
                );
            }
            let mut row = json!({"id":r.get::<_, String>(0)?,"path":path,"kind":r.get::<_, String>(1)?,
                "name":r.get::<_, String>(2)?,"line":r.get::<_, i64>(3)?,"column":r.get::<_, i64>(4)?,
                "startByte":r.get::<_, i64>(5)?,"endByte":r.get::<_, i64>(6)?});
            if let Some(target) = r.get::<_, Option<String>>(7)? {
                row["target"] = json!(target);
            }
            if let Some(container) = r.get::<_, Option<String>>(12)? {
                row["container"] = json!({"id":container,"kind":r.get::<_, String>(13)?,
                    "name":r.get::<_, String>(14)?,"line":r.get::<_, i64>(15)?});
            }
            results.push(row);
        }
        drop(rows);
        let more = results.len() > query.limit;
        results.truncate(query.limit);
        // The look-ahead row may have listed a source no returned row cites.
        sources.retain(|path, _| results.iter().any(|row| row["path"] == *path));
        let mut result = json!({"results":results,"sources":sources,
            "continuation":if more { results.last().map(|row| row["id"].clone()) } else { None },
            "sourcePath":query.source,"kind":one(query.kinds),"under":one(&under)});
        if more {
            result["matches"] = self.matches(&filters, filter)?;
        }
        result["coverage"] = self.status()?;
        Ok(result)
    }

    /// Total matches by kind and by directory, for narrowing a truncated query.
    fn matches(&self, filters: &str, filter: impl rusqlite::Params) -> Result<Value, Error> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT s.path,e.kind,count(*) FROM evidence e JOIN sources s USING(sid)
            WHERE {filters} GROUP BY s.path,e.kind ORDER BY s.path"
        ))?;
        let mut rows = statement.query(filter)?;
        let (mut total, mut kinds, mut paths) = (0u64, serde_json::Map::new(), Vec::new());
        while let Some(r) = rows.next()? {
            let (path, kind, count): (String, String, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            let count = count as u64;
            total += count;
            let entry = kinds.entry(kind).or_insert(json!(0));
            *entry = json!(entry.as_u64().unwrap_or(0) + count);
            match paths.last_mut() {
                Some((last, sum)) if *last == path => *sum += count,
                _ => paths.push((path, count)),
            }
        }
        let (under, directories) = craft_engine::directory_counts(&paths, 12);
        Ok(
            json!({"total":total,"kinds":kinds,"sources":paths.len(),"under":under,"directories":directories}),
        )
    }
}

/// A single filter value as given, or null when it is absent or a list.
fn one(items: &[&str]) -> Value {
    match items {
        [item] => json!(item),
        _ => Value::Null,
    }
}

/// One evidence lookup: an exact field match, narrowed before pagination.
pub struct Query<'a> {
    pub field: &'a str,
    pub value: &'a str,
    pub after: &'a str,
    pub limit: usize,
    /// One exact source path.
    pub source: Option<&'a str>,
    /// Exact evidence kinds; empty matches every kind.
    pub kinds: &'a [&'a str],
    /// Scopes: a source path or a directory, matched by whole path components; empty matches
    /// every path.
    pub under: &'a [&'a str],
}

impl<'a> Query<'a> {
    pub fn new(field: &'a str, value: &'a str) -> Self {
        Self {
            field,
            value,
            after: "",
            limit: 30,
            source: None,
            kinds: &[],
            under: &[],
        }
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
            store.query(&Query::new("name", "before")).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.query(&Query::new("name", "after")).unwrap()["results"],
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
        store.finish_inventory(epoch, &Value::Null, &[], 0).unwrap();
        assert_eq!(store.retain_providers(&[PROVIDER]).unwrap(), 0);
        assert_eq!(
            store.retain_providers(&["replacement-provider"]).unwrap(),
            1
        );
        assert_eq!(store.status().unwrap()["evidence"], 0);
        assert_eq!(store.status().unwrap()["inventoryComplete"], false);
        assert_eq!(
            store.query(&Query::new("name", "previous")).unwrap()["results"],
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
        assert_eq!(
            store
                .finish_inventory(restarted, &Value::Null, &[], 0)
                .unwrap(),
            0
        );
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
            store.query(&Query::new("name", "before")).unwrap()["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        store
            .fail("a.js", PROVIDER, "", "syntax error", "2", epoch)
            .unwrap();
        assert!(
            store.query(&Query::new("name", "before")).unwrap()["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.status().unwrap()["failedSources"], 1);
        store
            .replace(&extract("a.js", b"// empty").unwrap(), "3", epoch)
            .unwrap();
        assert_eq!(store.status().unwrap()["evidence"], 0);
        store.finish_inventory(epoch, &Value::Null, &[], 0).unwrap();
        let next = store.begin_inventory().unwrap();
        assert_eq!(
            store.status().unwrap()["sources"],
            1,
            "partial inventory cannot retire unseen sources"
        );
        assert_eq!(
            store.finish_inventory(next, &Value::Null, &[], 0).unwrap(),
            1
        );
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
        let page = store
            .query(&Query {
                limit: 1,
                ..Query::new("name", "a")
            })
            .unwrap();
        assert_eq!(page["results"][0]["name"], "a");
        assert!(
            store
                .query(&Query {
                    after: "not-an-id",
                    ..Query::new("name", "a")
                })
                .is_err()
        );
        drop(store);
        std::fs::remove_file(&database).unwrap();
    }

    #[test]
    fn queries_page_in_source_order_with_filters_containers_and_counts() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let epoch = store.begin_inventory().unwrap();
        for (path, source) in [
            (
                "b.java",
                "class B { void run() { ping(); } void ping() {} }",
            ),
            (
                "a/x.java",
                "class X {\n void go() {\n  ping();\n  ping();\n }\n}",
            ),
            ("a/y/z.java", "class Z { void go() { ping(); } }"),
            ("ab/w.java", "class W { void go() { ping(); } }"),
        ] {
            let extraction = crate::java::extract(path, source.as_bytes()).unwrap();
            store.replace(&extraction, "1", epoch).unwrap();
        }
        let page = |query: Query| store.query(&query).unwrap();
        let first = page(Query {
            limit: 2,
            ..Query::new("name", "ping")
        });
        let rows = first["results"].as_array().unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row["path"].as_str().unwrap(), row["line"].as_i64().unwrap()))
                .collect::<Vec<_>>(),
            [("a/x.java", 3), ("a/x.java", 4)]
        );
        assert_eq!(
            rows[0]["container"]
                .as_object()
                .map(|c| (&c["kind"], &c["name"], &c["line"])),
            Some((&json!("java:declares-method"), &json!("go"), &json!(2)))
        );
        assert_eq!(first["sources"].as_object().unwrap().len(), 1);
        assert_eq!(
            first["matches"],
            json!({"total":6,"kinds":{"java:invokes-syntax":5,"java:declares-method":1},
                "sources":4,"under":"","directories":{"a/":3,"b.java":2,"ab/":1}})
        );
        let mut all = Vec::new();
        let mut after = String::new();
        loop {
            let next = page(Query {
                limit: 2,
                after: &after,
                ..Query::new("name", "ping")
            });
            all.extend(next["results"].as_array().unwrap().clone());
            match next["continuation"].as_str() {
                Some(id) => after = id.to_string(),
                None => break,
            }
        }
        assert_eq!(
            all,
            page(Query::new("name", "ping"))["results"]
                .as_array()
                .unwrap()
                .clone()
        );
        assert!(page(Query::new("name", "ping")).get("matches").is_none());
        let kinds = page(Query {
            kinds: &["java:declares-method"],
            ..Query::new("name", "ping")
        });
        assert_eq!(kinds["results"][0]["path"], "b.java");
        assert_eq!(kinds["results"].as_array().unwrap().len(), 1);
        let either = page(Query {
            kinds: &["java:declares-method", "java:invokes-syntax"],
            under: &["a/y", "ab"],
            ..Query::new("name", "ping")
        });
        let paths: Vec<_> = either["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["a/y/z.java", "ab/w.java"]);
        assert_eq!(
            (&either["kind"], &either["under"]),
            (&json!(null), &json!(null))
        );
        for under in ["a", "a/"] {
            let scoped = page(Query {
                under: &[under],
                ..Query::new("name", "ping")
            });
            assert_eq!(scoped["under"], "a");
            assert!(
                scoped["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["path"].as_str().unwrap().starts_with("a/"))
            );
            assert_eq!(scoped["results"].as_array().unwrap().len(), 3);
        }
        for under in ["../a", "/a", ""] {
            assert!(
                store
                    .query(&Query {
                        under: &[under],
                        ..Query::new("name", "ping")
                    })
                    .is_err()
            );
        }
        let continuation = first["continuation"].as_str().unwrap().to_string();
        let replaced = crate::java::extract("a/x.java", b"class X {}").unwrap();
        store.replace(&replaced, "2", epoch).unwrap();
        let stale = store.query(&Query {
            after: &continuation,
            ..Query::new("name", "ping")
        });
        assert!(stale.unwrap_err().to_string().contains("restart the query"));
    }

    #[test]
    fn unknown_schema_refuses_writes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(Store::initialize(connection).is_err());
    }
}
