use super::*;
use crate::providers::javascript::{PROVIDER, extract};

#[test]
fn a_refused_write_ahead_log_keeps_the_rollback_journal() {
    let database =
        std::env::temp_dir().join(format!("craft-store-journal-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&database);
    let connection = Connection::open(&database).unwrap();
    // SQLite refuses the switch inside a transaction, as a filesystem without shared
    // memory does on open; the store stays usable on its default journal.
    connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
    assert!(!write_ahead(&connection).unwrap());
    connection.execute_batch("COMMIT;").unwrap();
    let mode: String = connection
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .unwrap();
    assert_eq!(synchronous, 2, "a rollback journal keeps full syncs");
    assert!(write_ahead(&connection).unwrap());
    drop(connection);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database.display()));
    }
}
#[test]
fn refused_write_preserves_previous_generation_and_facts() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let epoch = store.begin_inventory().unwrap();
    store
        .replace(
            &extract("a.js", b"const before = 1;").unwrap(),
            "1",
            epoch,
            None,
        )
        .unwrap();
    let generation = store.status().unwrap()["generation"].clone();
    store
        .connection
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert!(
        store
            .replace(
                &extract("a.js", b"const after = 1;").unwrap(),
                "2",
                epoch,
                None
            )
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
            None,
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
    store.replace(&extraction, "1", epoch, None).unwrap();
    assert_eq!(store.status().unwrap()["inventoryComplete"], false);
    let restarted = store.begin_inventory().unwrap();
    assert!(
        store
            .reuse("a.js", PROVIDER, &extraction.hash, None)
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
fn an_inventory_that_changes_nothing_writes_nothing() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let (a, b) = (
        extract("a.js", b"const a = 1;").unwrap(),
        extract("b.js", b"const b = 1;").unwrap(),
    );
    let epoch = store.begin_inventory().unwrap();
    store.begin_batch().unwrap();
    store.replace(&a, "1", epoch, None).unwrap();
    store.replace(&b, "1", epoch, None).unwrap();
    store.commit_batch().unwrap();
    let excluded = ["vendor/".to_string()];
    store
        .finish_inventory(epoch, &json!({"kind": "git"}), &excluded, 2)
        .unwrap();
    let before = store.status().unwrap();
    let changes = store.connection.total_changes();

    let epoch = store.begin_inventory().unwrap();
    store.begin_batch().unwrap();
    assert!(store.reuse("a.js", PROVIDER, &a.hash, None).unwrap());
    assert!(store.reuse("b.js", PROVIDER, &b.hash, None).unwrap());
    assert!(store.connection.is_autocommit(), "no write lock taken");
    store.commit_batch().unwrap();
    let removed = store
        .finish_inventory(epoch, &json!({"kind": "git"}), &excluded, 2)
        .unwrap();
    assert_eq!(removed, 0);
    assert_eq!(store.connection.total_changes(), changes);
    assert_eq!(store.status().unwrap(), before);

    // A pass interrupted before its first change leaves the complete inventory standing.
    store.begin_inventory().unwrap();
    assert!(store.reuse("a.js", PROVIDER, &a.hash, None).unwrap());
    assert_eq!(store.status().unwrap()["inventoryComplete"], true);

    // Changed exclusions alone are recorded; an unseen source alone is retired.
    let epoch = store.begin_inventory().unwrap();
    store.reuse("a.js", PROVIDER, &a.hash, None).unwrap();
    store.reuse("b.js", PROVIDER, &b.hash, None).unwrap();
    store
        .finish_inventory(epoch, &json!({"kind": "git"}), &[], 0)
        .unwrap();
    let status = store.status().unwrap();
    assert_eq!(
        (&status["exclusions"], &status["excludedSources"]),
        (&json!([]), &json!(0))
    );
    assert_eq!(
        status["generation"],
        before["generation"].as_i64().unwrap() + 1
    );
    let epoch = store.begin_inventory().unwrap();
    store.reuse("a.js", PROVIDER, &a.hash, None).unwrap();
    assert_eq!(
        store
            .finish_inventory(epoch, &json!({"kind": "git"}), &[], 0)
            .unwrap(),
        1
    );
    let status = store.status().unwrap();
    assert_eq!(
        (&status["sources"], &status["inventoryComplete"]),
        (&json!(1), &json!(true))
    );
    assert!(store.query(&Query::new("name", "b")).unwrap()["results"] == json!([]));
}

#[test]
fn a_settled_stamp_reuses_a_source_without_its_content() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let a = extract("a.js", b"const a = 1;").unwrap();
    let epoch = store.begin_inventory().unwrap();
    store.replace(&a, "1", epoch, None).unwrap();
    store.finish_inventory(epoch, &Value::Null, &[], 0).unwrap();

    // A source stored without a stamp is read; its settled stamp is then recorded.
    let epoch = store.begin_inventory().unwrap();
    assert!(!store.reuse_stamped("a.js", PROVIDER, "s1").unwrap());
    assert!(store.reuse("a.js", PROVIDER, &a.hash, Some("s1")).unwrap());
    store.finish_inventory(epoch, &Value::Null, &[], 0).unwrap();
    let changes = store.connection.total_changes();
    let epoch = store.begin_inventory().unwrap();
    assert!(store.reuse_stamped("a.js", PROVIDER, "s1").unwrap());
    assert_eq!(
        store.finish_inventory(epoch, &Value::Null, &[], 0).unwrap(),
        0
    );
    assert_eq!(store.connection.total_changes(), changes, "nothing written");

    // Another stamp is not this content; a replacement carries its own stamp.
    let epoch = store.begin_inventory().unwrap();
    assert!(!store.reuse_stamped("a.js", PROVIDER, "s2").unwrap());
    let b = extract("a.js", b"const b = 1;").unwrap();
    store.replace(&b, "2", epoch, Some("s2")).unwrap();
    assert!(!store.reuse_stamped("a.js", PROVIDER, "s1").unwrap());
    assert!(store.reuse_stamped("a.js", PROVIDER, "s2").unwrap());
    // Deferred work has no evidence for any stamp.
    store.defer("a.js", PROVIDER, epoch).unwrap();
    assert!(!store.reuse_stamped("a.js", PROVIDER, "s2").unwrap());
    store
        .fail(
            "a.js",
            PROVIDER,
            &b.hash,
            "syntax error",
            "3",
            epoch,
            Some("s3"),
        )
        .unwrap();
    assert!(
        store.reuse_stamped("a.js", PROVIDER, "s3").unwrap(),
        "failed content is not retried"
    );
}

#[test]
fn replacement_failure_empty_success_and_deletion_are_distinct() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let epoch = store.begin_inventory().unwrap();
    store
        .replace(
            &extract("a.js", b"const before = 1;").unwrap(),
            "1",
            epoch,
            None,
        )
        .unwrap();
    assert_eq!(
        store.query(&Query::new("name", "before")).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store
        .fail("a.js", PROVIDER, "", "syntax error", "2", epoch, None)
        .unwrap();
    assert!(
        store.query(&Query::new("name", "before")).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.status().unwrap()["failedSources"], 1);
    store
        .replace(&extract("a.js", b"// empty").unwrap(), "3", epoch, None)
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
                None,
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
            None,
        )
        .unwrap();
    store
        .replace(&extract("c.js", b"const c = 1;").unwrap(), "1", epoch, None)
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
fn partial_sources_answer_and_stay_listed_as_gaps() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let epoch = store.begin_inventory().unwrap();
    let extraction = crate::providers::java::extract(
        "A.java",
        b"class A { void go() { ping(); } void broken( }",
    )
    .unwrap();
    assert!(extraction.partial);
    store.replace(&extraction, "1", epoch, None).unwrap();
    let rows = store.query(&Query::new("name", "ping")).unwrap();
    assert_eq!(rows["results"][0]["container"]["name"], "go");
    assert_eq!(
        store.unextracted().unwrap(),
        [(
            "A.java".into(),
            crate::providers::java::PROVIDER.into(),
            "partial".into()
        )]
    );
    let status = store.status().unwrap();
    assert_eq!(
        (&status["partialSources"], &status["failedSources"]),
        (&json!(1), &json!(0))
    );
    assert!(
        store
            .reuse(
                "A.java",
                crate::providers::java::PROVIDER,
                &extraction.hash,
                None
            )
            .unwrap()
    );
}

#[test]
fn imports_match_names_and_trailing_module_segments() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let epoch = store.begin_inventory().unwrap();
    for (path, source) in [
        (
            "src/a.ts",
            "import { decode } from '../utils/buffer'\nimport { AdrSlug as Slug } from './AdrSlug.js'\n",
        ),
        (
            "src/b.ts",
            "import x from './my_buffer'\nexport * from 'utils/buffer'\n",
        ),
        (
            "h/c.py",
            "from ._urlparse import urlparse\nimport a_urlparse\n",
        ),
        ("g/C.java", "import com.google.gson.Gson;\nclass C {}"),
        ("r/lib.rs", "use crate::a::b;\nuse crate::ab;\n"),
    ] {
        let extraction = crate::providers::extract(path, source.as_bytes()).unwrap();
        store.replace(&extraction, "1", epoch, None).unwrap();
    }
    let found = |value: &str| -> Vec<(String, String)> {
        store.query(&Query::new("import", value)).unwrap()["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["path"].as_str().unwrap().into(),
                    r["kind"].as_str().unwrap().into(),
                )
            })
            .collect()
    };
    let pairs = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
        rows.iter()
            .map(|(p, k)| (p.to_string(), k.to_string()))
            .collect()
    };
    assert_eq!(
        found("utils/buffer"),
        pairs(&[("src/a.ts", "ts:imports"), ("src/b.ts", "ts:imports")])
    );
    assert_eq!(found("buffer"), found("utils/buffer"));
    assert_eq!(
        found("AdrSlug"),
        pairs(&[("src/a.ts", "ts:imports"), ("src/a.ts", "ts:imports-name")])
    );
    assert_eq!(found("_urlparse"), pairs(&[("h/c.py", "python:imports")]));
    assert_eq!(
        found("urlparse"),
        pairs(&[("h/c.py", "python:imports-name")])
    );
    assert_eq!(found("Gson"), pairs(&[("g/C.java", "java:imports")]));
    assert_eq!(found("b"), pairs(&[("r/lib.rs", "rust:imports")]));
    assert!(found("uffer").is_empty());
}

#[test]
fn mentions_match_as_spelled_or_by_their_last_qualified_segment() {
    let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let epoch = store.begin_inventory().unwrap();
    let source = "# A\n`craft_engine::Evidence`, `Gson.toJson()`, `Type#run`, `Evidence`, `MyEvidence`, `a_Evidence`.\n[up](/src/x.Evidence)\n";
    let extraction = crate::providers::extract("docs/a.md", source.as_bytes()).unwrap();
    store.replace(&extraction, "1", epoch, None).unwrap();
    let found = |value: &str| -> Vec<String> {
        store.query(&Query::new("mention", value)).unwrap()["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().into())
            .collect()
    };
    assert_eq!(found("Evidence"), ["craft_engine::Evidence", "Evidence"]);
    assert_eq!(found("toJson"), ["Gson.toJson"]);
    assert_eq!(found("run"), ["Type#run"]);
    // A qualified value matches its own spelling only, and a link only its whole path.
    assert_eq!(found("craft_engine::Evidence"), ["craft_engine::Evidence"]);
    assert_eq!(found("src/x.Evidence"), ["src/x.Evidence"]);
    assert!(found("engine::Evidence").is_empty());
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
        let extraction = crate::providers::java::extract(path, source.as_bytes()).unwrap();
        store.replace(&extraction, "1", epoch, None).unwrap();
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
    let replaced = crate::providers::java::extract("a/x.java", b"class X {}").unwrap();
    store.replace(&replaced, "2", epoch, None).unwrap();
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
