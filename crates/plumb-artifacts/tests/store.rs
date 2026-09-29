use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

use plumb_artifacts::{
    ensure_artifact_schema, put_artifact_in_transaction, ArtifactKind, ArtifactStore,
    ArtifactStoreError, SqliteArtifactStore, UnknownArtifactKind,
};
use plumb_core::{Hash, HashKind, Timestamp};
use rusqlite::{Connection, TransactionBehavior};
use serde::de::value::{Error as ValueError, StrDeserializer};
use serde::de::IntoDeserializer;
use serde::Deserialize;
use tempfile::TempDir;

const T1: &str = "2026-09-29T12:00:00.000000001Z";
const T2: &str = "2026-09-30T08:15:00.500000000Z";
const JSON: &str = "application/json";

fn ts(s: &str) -> Timestamp {
    s.parse().unwrap()
}

struct Db {
    _dir: TempDir,
    path: PathBuf,
}

impl Db {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plumb.sqlite");
        Db { _dir: dir, path }
    }

    fn store(&self) -> SqliteArtifactStore {
        SqliteArtifactStore::open(&self.path).unwrap()
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
}

fn row_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM artifacts", [], |r| r.get(0))
        .unwrap()
}

/// The raw persisted row (hash, kind, media_type, bytes, created_at) for `hash`.
fn raw_row(path: &Path, hash: &Hash) -> (String, String, String, Vec<u8>, String) {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT hash, kind, media_type, bytes, created_at FROM artifacts WHERE hash = ?1",
            [hash.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
}

// 1
#[test]
fn first_put_inserts_exactly_one_row() {
    let db = Db::new();
    let mut store = db.store();
    assert_eq!(row_count(&db.path), 0);
    store
        .put(
            ArtifactKind::SourceOriginal,
            "text/markdown",
            b"# HR",
            ts(T1),
        )
        .unwrap();
    assert_eq!(row_count(&db.path), 1);
}

// 2
#[test]
fn identical_put_twice_returns_the_same_hash() {
    let db = Db::new();
    let mut store = db.store();
    let first = store
        .put(ArtifactKind::Patch, JSON, b"{\"op\":1}", ts(T1))
        .unwrap();
    let second = store
        .put(ArtifactKind::Patch, JSON, b"{\"op\":1}", ts(T1))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(row_count(&db.path), 1);
}

// 3
#[test]
fn duplicate_put_keeps_the_original_created_at() {
    let db = Db::new();
    let mut store = db.store();
    let hash = store
        .put(ArtifactKind::Diff, JSON, b"diff", ts(T1))
        .unwrap();
    assert_eq!(
        store
            .put(ArtifactKind::Diff, JSON, b"diff", ts(T2))
            .unwrap(),
        hash
    );
    assert_eq!(store.get(&hash).unwrap().created_at, ts(T1));
    assert_eq!(raw_row(&db.path, &hash).4, T1);
}

// 4
#[test]
fn different_bytes_produce_different_hashes_and_rows() {
    let db = Db::new();
    let mut store = db.store();
    let a = store
        .put(ArtifactKind::Projection, JSON, b"a", ts(T1))
        .unwrap();
    let b = store
        .put(ArtifactKind::Projection, JSON, b"b", ts(T1))
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(row_count(&db.path), 2);
}

// 5
#[test]
fn same_bytes_with_a_different_kind_is_a_metadata_conflict() {
    let db = Db::new();
    let mut store = db.store();
    let hash = store
        .put(ArtifactKind::InferenceRequest, JSON, b"req", ts(T1))
        .unwrap();
    match store.put(ArtifactKind::InferenceResponse, JSON, b"req", ts(T2)) {
        Err(ArtifactStoreError::ArtifactMetadataConflict {
            hash: h,
            existing_kind,
            existing_media_type,
            requested_kind,
            requested_media_type,
        }) => {
            assert_eq!(h, hash);
            assert_eq!(existing_kind, ArtifactKind::InferenceRequest);
            assert_eq!(existing_media_type, JSON);
            assert_eq!(requested_kind, ArtifactKind::InferenceResponse);
            assert_eq!(requested_media_type, JSON);
        }
        other => panic!("expected ArtifactMetadataConflict, got {other:?}"),
    }
}

// 6
#[test]
fn same_bytes_with_a_different_media_type_is_a_metadata_conflict() {
    let db = Db::new();
    let mut store = db.store();
    store
        .put(ArtifactKind::GateReport, JSON, b"report", ts(T1))
        .unwrap();
    assert!(matches!(
        store.put(ArtifactKind::GateReport, "text/plain", b"report", ts(T1)),
        Err(ArtifactStoreError::ArtifactMetadataConflict { .. })
    ));
}

// 7
#[test]
fn metadata_conflict_leaves_the_original_row_byte_for_byte_unchanged() {
    let db = Db::new();
    let mut store = db.store();
    let hash = store
        .put(ArtifactKind::ScenarioTrace, JSON, b"trace", ts(T1))
        .unwrap();
    let before = raw_row(&db.path, &hash);
    for (kind, media) in [
        (ArtifactKind::TestReceipt, JSON),
        (ArtifactKind::ScenarioTrace, "text/plain"),
        (ArtifactKind::TestReceipt, "text/plain"),
    ] {
        assert!(store.put(kind, media, b"trace", ts(T2)).is_err());
    }
    assert_eq!(raw_row(&db.path, &hash), before);
    assert_eq!(row_count(&db.path), 1);
}

// 8
#[test]
fn get_returns_all_five_persisted_fields() {
    let db = Db::new();
    let mut store = db.store();
    let bytes = b"\x00\x01binary\xffpayload".to_vec();
    let hash = store
        .put(
            ArtifactKind::EvidenceManifest,
            "application/octet-stream",
            &bytes,
            ts("2026-09-29T14:00:00.123456789+02:00"),
        )
        .unwrap();
    let artifact = db.store().get(&hash).unwrap();
    assert_eq!(artifact.hash, hash);
    assert_eq!(artifact.kind, ArtifactKind::EvidenceManifest);
    assert_eq!(artifact.media_type, "application/octet-stream");
    assert_eq!(artifact.bytes, bytes);
    assert_eq!(artifact.created_at, ts("2026-09-29T12:00:00.123456789Z"));
    // created_at is persisted in the canonical UTC form.
    assert_eq!(raw_row(&db.path, &hash).4, "2026-09-29T12:00:00.123456789Z");
}

// 9
#[test]
fn exists_is_true_only_for_stored_hashes() {
    let db = Db::new();
    let mut store = db.store();
    let hash = store
        .put(ArtifactKind::Proposal, JSON, b"p", ts(T1))
        .unwrap();
    assert!(store.exists(&hash).unwrap());
    let missing = Hash::content_sha256(b"never stored");
    assert!(!store.exists(&missing).unwrap());
    assert!(matches!(
        store.get(&missing),
        Err(ArtifactStoreError::NotFound(h)) if h == missing
    ));
}

// 10
#[test]
fn artifact_hash_is_the_generic_sha256_of_the_exact_bytes() {
    let db = Db::new();
    let mut store = db.store();
    let hash = store
        .put(ArtifactKind::SourceExtracted, "text/plain", b"abc", ts(T1))
        .unwrap();
    assert_eq!(hash.kind(), HashKind::Generic);
    assert_eq!(
        hash.as_str(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(hash, Hash::content_sha256(b"abc"));
}

// 11
#[test]
fn semantic_hash_keys_are_rejected() {
    let db = Db::new();
    let mut store = db.store();
    store
        .put(ArtifactKind::SourceExtracted, "text/plain", b"abc", ts(T1))
        .unwrap();
    let psg = Hash::semantic_sha256(b"abc");
    assert!(matches!(store.get(&psg), Err(ArtifactStoreError::NonGenericHash(h)) if h == psg));
    assert!(matches!(
        store.exists(&psg),
        Err(ArtifactStoreError::NonGenericHash(_))
    ));
}

// 12
#[test]
fn evidence_hash_keys_are_rejected() {
    let db = Db::new();
    let mut store = db.store();
    store
        .put(ArtifactKind::SourceExtracted, "text/plain", b"abc", ts(T1))
        .unwrap();
    let ev = Hash::evidence_sha256(b"abc");
    assert!(matches!(store.get(&ev), Err(ArtifactStoreError::NonGenericHash(h)) if h == ev));
    assert!(matches!(
        store.exists(&ev),
        Err(ArtifactStoreError::NonGenericHash(_))
    ));
}

// 13
#[test]
fn unknown_persisted_kind_fails_explicitly() {
    let db = Db::new();
    let store = db.store();
    let hash = Hash::content_sha256(b"legacy");
    db.raw()
        .execute(
            "INSERT INTO artifacts (hash, kind, media_type, bytes, created_at) \
             VALUES (?1, 'other', 'text/plain', ?2, ?3)",
            rusqlite::params![hash.as_str(), b"legacy".to_vec(), T1],
        )
        .unwrap();
    let before = raw_row(&db.path, &hash);
    assert!(matches!(
        store.get(&hash),
        Err(ArtifactStoreError::UnknownArtifactKind(UnknownArtifactKind(k))) if k == "other"
    ));
    let mut store = store;
    assert!(matches!(
        store.put(ArtifactKind::Patch, "text/plain", b"legacy", ts(T2)),
        Err(ArtifactStoreError::UnknownArtifactKind(_))
    ));
    assert_eq!(raw_row(&db.path, &hash), before);
}

// 14
#[test]
fn repeated_insertion_never_creates_more_than_one_row() {
    let db = Db::new();
    let mut store = db.store();
    let first = store
        .put(ArtifactKind::ImpactReport, JSON, b"impact", ts(T1))
        .unwrap();
    for _ in 0..25 {
        assert_eq!(
            store
                .put(ArtifactKind::ImpactReport, JSON, b"impact", ts(T2))
                .unwrap(),
            first
        );
    }
    assert_eq!(row_count(&db.path), 1);
}

#[test]
fn concurrent_puts_on_separate_connections_converge() {
    let db = Db::new();
    db.store(); // create the schema before the race
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|i| {
            let path = db.path.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut store = SqliteArtifactStore::open(&path).unwrap();
                // Even threads agree on the metadata; odd threads race with a different kind.
                let kind = if i % 2 == 0 {
                    ArtifactKind::ValidatedInference
                } else {
                    ArtifactKind::ExternalValidation
                };
                barrier.wait();
                (kind, store.put(kind, JSON, b"raced bytes", ts(T1)))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(row_count(&db.path), 1);
    let winner = db
        .store()
        .get(&Hash::content_sha256(b"raced bytes"))
        .unwrap()
        .kind;
    for (kind, result) in results {
        if kind == winner {
            assert_eq!(result.unwrap(), Hash::content_sha256(b"raced bytes"));
        } else {
            assert!(matches!(
                result,
                Err(ArtifactStoreError::ArtifactMetadataConflict { .. })
            ));
        }
    }
}

// 15
#[test]
fn failed_put_rolls_back_and_leaves_no_partial_artifact() {
    let db = Db::new();
    let mut store = db.store();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_after_insert AFTER INSERT ON artifacts \
             BEGIN SELECT RAISE(ABORT, 'forced failure'); END;",
        )
        .unwrap();
    let hash = Hash::content_sha256(b"doomed");
    assert!(matches!(
        store.put(ArtifactKind::ConformanceReport, JSON, b"doomed", ts(T1)),
        Err(ArtifactStoreError::Sqlite(_))
    ));
    assert_eq!(row_count(&db.path), 0);
    assert!(!store.exists(&hash).unwrap());
    db.raw()
        .execute_batch("DROP TRIGGER fail_after_insert;")
        .unwrap();
    assert_eq!(
        store
            .put(ArtifactKind::ConformanceReport, JSON, b"doomed", ts(T1))
            .unwrap(),
        hash
    );
    assert_eq!(row_count(&db.path), 1);
}

#[test]
fn tampered_stored_bytes_are_an_integrity_error_and_are_not_overwritten() {
    // Simulates storage corruption (not a SHA-256 collision): the row keyed by the hash of
    // "genuine" holds different bytes.
    let db = Db::new();
    let mut store = db.store();
    let hash = Hash::content_sha256(b"genuine");
    db.raw()
        .execute(
            "INSERT INTO artifacts (hash, kind, media_type, bytes, created_at) \
             VALUES (?1, 'patch', 'application/json', ?2, ?3)",
            rusqlite::params![hash.as_str(), b"tampered".to_vec(), T1],
        )
        .unwrap();
    let before = raw_row(&db.path, &hash);
    assert!(matches!(
        store.put(ArtifactKind::Patch, JSON, b"genuine", ts(T2)),
        Err(ArtifactStoreError::IntegrityViolation(h)) if h == hash
    ));
    assert_eq!(raw_row(&db.path, &hash), before);
}

#[test]
fn reopening_the_store_preserves_artifacts() {
    let db = Db::new();
    let hash = db
        .store()
        .put(ArtifactKind::ArchitectureCheck, JSON, b"check", ts(T1))
        .unwrap();
    let reopened = db.store();
    assert!(reopened.exists(&hash).unwrap());
    assert_eq!(reopened.get(&hash).unwrap().bytes, b"check");
}

#[test]
fn in_memory_store_supports_put_get_exists() {
    let mut store = SqliteArtifactStore::open_in_memory().unwrap();
    let hash = store
        .put(ArtifactKind::SourceOriginal, "text/plain", b"mem", ts(T1))
        .unwrap();
    assert!(store.exists(&hash).unwrap());
    assert_eq!(store.get(&hash).unwrap().media_type, "text/plain");
}

#[test]
fn artifact_kinds_use_exactly_the_specified_strings() {
    let expected = [
        "source-original",
        "source-extracted",
        "evidence-manifest",
        "inference-request",
        "inference-response",
        "validated-inference",
        "external-validation",
        "projection",
        "proposal",
        "patch",
        "diff",
        "impact-report",
        "gate-report",
        "conformance-report",
        "scenario-trace",
        "test-receipt",
        "architecture-check",
    ];
    assert_eq!(ArtifactKind::ALL.map(ArtifactKind::as_str), expected);
    for (kind, text) in ArtifactKind::ALL.into_iter().zip(expected) {
        assert_eq!(kind.to_string(), text);
        assert_eq!(text.parse::<ArtifactKind>().unwrap(), kind);
        let de: StrDeserializer<ValueError> = text.into_deserializer();
        assert_eq!(ArtifactKind::deserialize(de).unwrap(), kind);
    }
    for bad in ["", "other", "Patch", "source_original", "source-original "] {
        assert_eq!(
            bad.parse::<ArtifactKind>(),
            Err(UnknownArtifactKind(bad.to_owned()))
        );
        let de: StrDeserializer<ValueError> = bad.into_deserializer();
        assert!(ArtifactKind::deserialize(de).is_err());
    }
}

#[test]
fn put_in_external_transaction_rolls_back_with_the_caller() {
    let db = Db::new();
    let mut conn = db.raw();
    ensure_artifact_schema(&conn).unwrap();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let hash =
        put_artifact_in_transaction(&tx, ArtifactKind::Patch, JSON, b"{\"a\":1}", ts(T1)).unwrap();
    assert_eq!(hash, Hash::content_sha256(b"{\"a\":1}"));
    let visible: i64 = tx
        .query_row("SELECT COUNT(*) FROM artifacts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(visible, 1);
    tx.rollback().unwrap();
    drop(conn);
    assert_eq!(row_count(&db.path), 0);
    assert!(!db.store().exists(&hash).unwrap());
}

#[test]
fn put_in_external_transaction_persists_when_the_caller_commits() {
    let db = Db::new();
    let mut conn = db.raw();
    ensure_artifact_schema(&conn).unwrap();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let hash =
        put_artifact_in_transaction(&tx, ArtifactKind::Patch, JSON, b"{\"a\":1}", ts(T1)).unwrap();
    // Idempotent inside the same transaction; a metadata conflict is still typed.
    assert_eq!(
        put_artifact_in_transaction(&tx, ArtifactKind::Patch, JSON, b"{\"a\":1}", ts(T2)).unwrap(),
        hash
    );
    assert!(matches!(
        put_artifact_in_transaction(&tx, ArtifactKind::Diff, JSON, b"{\"a\":1}", ts(T2)),
        Err(ArtifactStoreError::ArtifactMetadataConflict { .. })
    ));
    tx.commit().unwrap();
    drop(conn);
    let stored = db.store().get(&hash).unwrap();
    assert_eq!(stored.kind, ArtifactKind::Patch);
    assert_eq!(stored.media_type, JSON);
    assert_eq!(stored.bytes, b"{\"a\":1}");
    assert_eq!(stored.created_at, ts(T1));
}

#[test]
fn ensure_artifact_schema_preserves_existing_artifacts() {
    let db = Db::new();
    let hash = db
        .store()
        .put(ArtifactKind::Patch, JSON, b"kept", ts(T1))
        .unwrap();
    let before = raw_row(&db.path, &hash);
    ensure_artifact_schema(&db.raw()).unwrap();
    assert_eq!(raw_row(&db.path, &hash), before);
}
