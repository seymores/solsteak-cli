use rusqlite::Connection;
use ssteak::storage::Store;
use std::sync::atomic::{AtomicU64, Ordering};

fn path() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ssteak-schema-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn creates_versioned_schema_with_exact_unsigned_values_and_foreign_keys() {
    assert_eq!(
        include_str!("../migrations/001_initial.sql"),
        include_str!("../docs/contracts/storage.sql")
    );
    let path = path();
    let _store = Store::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    conn.execute_batch("PRAGMA foreign_keys=ON; INSERT INTO networks VALUES ('network','mainnet','18446744073709551615',NULL);").unwrap();
    for amount in [0_u64, 1, i64::MAX as u64, u64::MAX] {
        let text = amount.to_string();
        conn.execute(
            "INSERT INTO rewards VALUES ('network',?1,?1,?1,?1,?1,NULL,'{}','{}')",
            [&text],
        )
        .unwrap();
        let saved: String = conn
            .query_row(
                "SELECT amount FROM rewards WHERE address=?1",
                [&text],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved.parse::<u64>().unwrap(), amount);
    }
    for bad in ["", "00", "-1", "1.5", "18446744073709551616"] {
        assert!(
            conn.execute(
                "INSERT INTO rewards VALUES ('network','bad',?1,?1,?1,?1,NULL,'{}','{}')",
                [bad]
            )
            .is_err()
        );
    }
    assert!(
        conn.execute("INSERT INTO members VALUES (999,'orphan',NULL)", [])
            .is_err()
    );
    assert!(
        conn.execute("INSERT INTO rewards SELECT * FROM rewards", [])
            .is_err()
    );
}

#[test]
fn refuses_newer_schema_without_modifying_file() {
    let path = path();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA user_version=99; CREATE TABLE future(value TEXT); INSERT INTO future VALUES ('preserve');").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(Store::open(&path).unwrap_err().code(), "UNSUPPORTED_SCHEMA");
    assert_eq!(before, std::fs::read(&path).unwrap());
}
