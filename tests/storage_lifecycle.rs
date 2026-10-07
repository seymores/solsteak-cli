use rusqlite::Connection;
use ssteak::storage::{Store, default_path};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir()
        .join(format!(
            "ssteak-life-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .join("db.sqlite3")
}
fn backup(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.pre-v0.sqlite3", path.display()))
}
#[test]
fn creates_private_location_and_platform_default() {
    let path = path();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.path(), path);
    assert!(default_path().unwrap().ends_with("ssteak/ssteak.sqlite3"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
#[test]
fn upgrade_backup_includes_committed_wal_data() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = Connection::open(&path).unwrap();
    original.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE prior(value TEXT); INSERT INTO prior VALUES ('committed WAL value');").unwrap();
    let _store = Store::open(&path).unwrap();
    let backup = Connection::open(backup(&path)).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT value FROM prior", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "committed WAL value"
    );
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
}
#[test]
fn failed_migration_rolls_back_and_preserves_original_and_backup() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = Connection::open(&path).unwrap();
    original.execute_batch("CREATE TABLE schema_history(original TEXT); INSERT INTO schema_history VALUES ('keep');").unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(
        original
            .query_row("SELECT original FROM schema_history", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        original
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert!(backup(&path).exists());
    assert_eq!(
        original
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='networks'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}
#[test]
fn corrupt_and_readonly_files_are_not_reset() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"broken sqlite file").unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"broken sqlite file");
    let path = path.with_file_name("readonly.sqlite3");
    drop(Store::open(&path).unwrap());
    let mut mode = std::fs::metadata(&path).unwrap().permissions();
    mode.set_readonly(true);
    std::fs::set_permissions(&path, mode).unwrap();
    assert!(Store::open(&path).is_err());
}

#[test]
fn concurrent_upgraders_keep_a_preupgrade_backup() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = Connection::open(&path).unwrap();
    original.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE prior(value BLOB); INSERT INTO prior VALUES (zeroblob(4000000));").unwrap();
    let gate = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let mut handles = vec![];
        for _ in 0..8 {
            let path = &path;
            let gate = &gate;
            handles.push(scope.spawn(move || {
                gate.wait();
                Store::open(path)
            }));
        }
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });
    let backup = Connection::open(backup(&path)).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        backup
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='networks'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}
