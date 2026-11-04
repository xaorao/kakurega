use kk_vault::{Vault, VaultError, VaultKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("os randomness");
    b
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-vault-{tag}-{}", hex(&random_bytes::<6>())));
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn canary() -> String {
    format!("CANARY-{}-secret-payload", hex(&random_bytes::<12>()))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn forms(canary: &str) -> Vec<Vec<u8>> {
    let utf16: Vec<u8> = canary
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    vec![
        canary.as_bytes().to_vec(),
        utf16,
        hex(canary.as_bytes()).into_bytes(),
    ]
}

fn scan_dir(dir: &Path, canary: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            let data = fs::read(&path).expect("read file");
            for f in forms(canary) {
                if contains(&data, &f) {
                    hits.push(path.display().to_string());
                    break;
                }
            }
        }
    }
    hits
}

fn init_schema(v: &Vault) {
    v.connection()
        .execute_batch(
            "CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
             CREATE VIRTUAL TABLE notes_fts USING fts5(body, content='notes', content_rowid='id');
             CREATE INDEX notes_body ON notes(body);",
        )
        .expect("schema");
}

fn insert_note(v: &Vault, body: &str) {
    let c = v.connection();
    c.execute("INSERT INTO notes(body) VALUES (?1)", [body])
        .expect("insert");
    let id = c.last_insert_rowid();
    c.execute(
        "INSERT INTO notes_fts(rowid, body) VALUES (?1, ?2)",
        rusqlite::params![id, body],
    )
    .expect("fts insert");
}

#[test]
fn vault_is_not_a_plain_sqlite_file_and_needs_the_right_key() {
    let dir = fresh_dir("keys");
    let path = dir.join("v.db");
    let key = VaultKey::from_bytes(random_bytes());
    {
        let v = Vault::create(&path, &key).expect("create");
        init_schema(&v);
        insert_note(&v, "hello");
        println!("cipher_version = {}", v.cipher_version().expect("version"));
    }
    let head = fs::read(&path).expect("read");
    assert!(
        !head.starts_with(b"SQLite format 3\0"),
        "plaintext header on disk"
    );

    let wrong = VaultKey::from_bytes(random_bytes());
    assert!(matches!(
        Vault::open(&path, &wrong),
        Err(VaultError::WrongKeyOrNotAVault)
    ));
    assert!(Vault::open(&path, &key).is_ok());
    assert!(matches!(
        Vault::open(&dir.join("absent.db"), &key),
        Err(VaultError::Missing)
    ));
    assert!(matches!(
        Vault::create(&path, &key),
        Err(VaultError::AlreadyExists)
    ));

    let plain = rusqlite::Connection::open(&path).expect("open plain");
    assert!(plain
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
            .get::<_, i64>(0))
        .is_err());
}

#[test]
fn canary_is_absent_from_every_file_after_import_search_and_checkpoint() {
    let dir = fresh_dir("canary");
    let path = dir.join("v.db");
    let key = VaultKey::from_bytes(random_bytes());
    let c = canary();
    {
        let mut v = Vault::create(&path, &key).expect("create");
        init_schema(&v);
        for i in 0..400 {
            insert_note(&v, &format!("{c} row {i} {}", "x".repeat(200)));
        }
        let hits: i64 = v
            .connection()
            .query_row(
                "SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'secret'",
                [],
                |r| r.get(0),
            )
            .expect("fts search");
        assert_eq!(hits, 400);
        let sorted: String = v
            .connection()
            .query_row(
                "SELECT body FROM notes ORDER BY body DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("sort");
        assert!(sorted.contains(&c));
        assert!(
            fs::metadata(dir.join("v.db-wal")).is_ok(),
            "expected a WAL file while open"
        );
        assert!(
            scan_dir(&dir, &c).is_empty(),
            "canary found while the vault is open: {:?}",
            scan_dir(&dir, &c)
        );
        v.connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("checkpoint");
        let backup = dir.join("backup.db");
        v.backup_to(&backup).expect("backup");
    }
    let leaks = scan_dir(&dir, &c);
    assert!(leaks.is_empty(), "plaintext canary found in: {leaks:?}");
    let backup_key_ok =
        Vault::open(&dir.join("backup.db"), &key).expect("backup opens with the same key");
    let n: i64 = backup_key_ok
        .connection()
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .expect("count");
    assert_eq!(n, 400);
}

fn run_child(
    test: &str,
    dir: &Path,
    tmp: &Path,
    key: &[u8; 32],
    canary: &str,
    extra: &[(&str, &str)],
) -> std::process::ExitStatus {
    let mut cmd = Command::new(std::env::current_exe().expect("exe"));
    cmd.args(["--exact", test, "--ignored", "--nocapture"])
        .env("KK_CHILD_DIR", dir)
        .env("KK_CHILD_KEY", hex(key))
        .env("KK_CHILD_CANARY", canary)
        .env("SQLITE_TMPDIR", tmp)
        .env("TMPDIR", tmp);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.status().expect("spawn child")
}

fn child_env() -> (PathBuf, VaultKey, String) {
    let dir = PathBuf::from(std::env::var("KK_CHILD_DIR").expect("dir"));
    let hexkey = std::env::var("KK_CHILD_KEY").expect("key");
    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = u8::from_str_radix(&hexkey[i * 2..i * 2 + 2], 16).expect("hex");
    }
    (
        dir,
        VaultKey::from_bytes(key),
        std::env::var("KK_CHILD_CANARY").expect("canary"),
    )
}

#[test]
#[ignore]
fn child_sort_workload() {
    let (dir, key, c) = child_env();
    let v = Vault::create(&dir.join("v.db"), &key).expect("create");
    init_schema(&v);
    v.connection()
        .execute_batch("PRAGMA cache_size = 16; PRAGMA temp.cache_size = 8;")
        .expect("cache");
    v.connection().execute_batch("BEGIN;").expect("begin");
    for i in 0..30000 {
        insert_note(&v, &format!("{c} row {i} {}", "y".repeat(600)));
    }
    v.connection().execute_batch("COMMIT;").expect("commit");
    v.connection()
        .execute_batch("CREATE TEMP TABLE big AS SELECT body || 'z' AS b FROM notes; CREATE INDEX temp.big_b ON big(b);")
        .expect("temp table");
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM (SELECT body || 'z' AS b FROM notes ORDER BY b DESC, length(body) DESC)",
            [],
            |r| r.get(0),
        )
        .expect("sort");
    assert_eq!(n, 30000);
    let g: i64 = v
        .connection()
        .query_row(
            "SELECT count(DISTINCT substr(body, 1, 400)) FROM notes",
            [],
            |r| r.get(0),
        )
        .expect("distinct");
    assert!(g > 0);
}

#[test]
#[ignore]
fn child_crash_workload() {
    let (dir, key, c) = child_env();
    let v = Vault::create(&dir.join("v.db"), &key).expect("create");
    init_schema(&v);
    for i in 0..200 {
        insert_note(&v, &format!("{c} committed {i}"));
    }
    v.connection().execute_batch("BEGIN;").expect("begin");
    for i in 0..200 {
        insert_note(&v, &format!("{c} uncommitted {i} {}", "u".repeat(300)));
    }
    std::process::abort();
}

#[test]
fn large_sort_does_not_spill_plaintext_into_the_temp_directory() {
    let dir = fresh_dir("temp");
    let tmp = dir.join("tmp");
    fs::create_dir_all(&tmp).expect("tmp");
    let c = canary();
    let key: [u8; 32] = random_bytes();
    assert!(run_child("child_sort_workload", &dir, &tmp, &key, &c, &[]).success());
    assert!(scan_dir(&tmp, &c).is_empty(), "temp dir holds plaintext");
    assert!(scan_dir(&dir, &c).is_empty(), "data dir holds plaintext");
}

#[test]
fn scanner_control_finds_the_canary_in_a_plain_unencrypted_database() {
    let dir = fresh_dir("control");
    let c = canary();
    let conn = rusqlite::Connection::open(dir.join("plain.db")).expect("plain db");
    conn.execute_batch("CREATE TABLE t(b TEXT);")
        .expect("schema");
    conn.execute("INSERT INTO t VALUES (?1)", [&c])
        .expect("insert");
    drop(conn);
    assert_eq!(
        scan_dir(&dir, &c).len(),
        1,
        "the scanner must see plaintext when it exists"
    );
}

#[test]
fn application_code_cannot_move_temporary_data_to_files_or_change_the_key() {
    let dir = fresh_dir("guard");
    let key = VaultKey::from_bytes(random_bytes());
    let v = Vault::create(&dir.join("v.db"), &key).expect("create");
    let mode: i64 = v
        .connection()
        .query_row("PRAGMA temp_store", [], |r| r.get(0))
        .expect("read temp_store");
    assert_eq!(mode, 2, "temporary data must be held in memory");
    for sql in [
        "PRAGMA temp_store = FILE",
        "PRAGMA temp_store = 1",
        "PRAGMA temp_store_directory = '/tmp'",
        "PRAGMA rekey = \"x'00'\"",
        "PRAGMA cipher_memory_security = OFF",
    ] {
        assert!(
            v.connection().execute_batch(sql).is_err(),
            "should be refused: {sql}"
        );
    }
    let other = dir.join("other.db");
    assert!(v
        .connection()
        .execute_batch(&format!(
            "ATTACH DATABASE '{}' AS x KEY ''",
            other.display()
        ))
        .is_err());
    let mode_after: i64 = v
        .connection()
        .query_row("PRAGMA temp_store", [], |r| r.get(0))
        .expect("read again");
    assert_eq!(mode_after, 2);
}

#[test]
fn crash_midway_leaves_no_plaintext_and_recovers_committed_rows_only() {
    let dir = fresh_dir("crash");
    let tmp = dir.join("tmp");
    fs::create_dir_all(&tmp).expect("tmp");
    let c = canary();
    let key_bytes: [u8; 32] = random_bytes();
    let status = run_child("child_crash_workload", &dir, &tmp, &key_bytes, &c, &[]);
    assert!(!status.success(), "child should have aborted");
    assert!(
        scan_dir(&dir, &c).is_empty(),
        "plaintext after crash: {:?}",
        scan_dir(&dir, &c)
    );
    assert!(scan_dir(&tmp, &c).is_empty());
    let v = Vault::open(&dir.join("v.db"), &VaultKey::from_bytes(key_bytes))
        .expect("reopen after crash");
    let total: i64 = v
        .connection()
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .expect("count");
    let uncommitted: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM notes WHERE body LIKE '%uncommitted%'",
            [],
            |r| r.get(0),
        )
        .expect("count u");
    assert_eq!(total, 200, "only committed rows survive");
    assert_eq!(uncommitted, 0);
    let ok: String = v
        .connection()
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .expect("integrity");
    assert_eq!(ok, "ok");
}

#[test]
fn rekey_invalidates_the_old_key_and_leaves_no_plaintext() {
    let dir = fresh_dir("rekey");
    let path = dir.join("v.db");
    let old = VaultKey::from_bytes(random_bytes());
    let c = canary();
    {
        let mut v = Vault::create(&path, &old).expect("create");
        init_schema(&v);
        insert_note(&v, &c);
        let new = VaultKey::from_bytes([7u8; 32]);
        v.rekey(&new).expect("rekey");
    }
    assert!(matches!(
        Vault::open(&path, &old),
        Err(VaultError::WrongKeyOrNotAVault)
    ));
    let v = Vault::open(&path, &VaultKey::from_bytes([7u8; 32])).expect("new key opens");
    let n: i64 = v
        .connection()
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .expect("count");
    assert_eq!(n, 1);
    assert!(scan_dir(&dir, &c).is_empty());
}

#[test]
fn a_corrupted_page_is_detected_not_silently_read() {
    let dir = fresh_dir("corrupt");
    let path = dir.join("v.db");
    let key = VaultKey::from_bytes(random_bytes());
    {
        let v = Vault::create(&path, &key).expect("create");
        init_schema(&v);
        for i in 0..300 {
            insert_note(&v, &format!("row {i} {}", "p".repeat(100)));
        }
        v.connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("checkpoint");
    }
    let mut data = fs::read(&path).expect("read");
    let mid = data.len() / 2;
    for b in &mut data[mid..mid + 64] {
        *b ^= 0xff;
    }
    fs::write(&path, &data).expect("write corrupted");
    let outcome = Vault::open(&path, &key).and_then(|v| {
        v.connection()
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .map_err(VaultError::from)
            .and_then(|s| {
                if s == "ok" {
                    Ok(())
                } else {
                    Err(VaultError::WrongKeyOrNotAVault)
                }
            })
            .and_then(|_| {
                let mut stmt = v.connection().prepare("SELECT body FROM notes")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                for r in rows {
                    r?;
                }
                Ok(())
            })
    });
    assert!(outcome.is_err(), "corruption went unnoticed");
}
