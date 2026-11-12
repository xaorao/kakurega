//! A store and its journal files are readable by their owner only, whether the store is new or was made before that rule.
use kk_vault::{Vault, VaultKey};
use std::os::unix::fs::PermissionsExt;

fn key() -> VaultKey {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("os randomness");
    VaultKey::from_bytes(b)
}

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn va_stores_are_owner_only_when_made_and_when_opened() {
    let dir = std::env::temp_dir().join(format!("kk-vault-mode-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let k = key();
    let path = dir.join("store.db");
    {
        let v = Vault::create(&path, &k).unwrap();
        v.connection()
            .execute_batch("CREATE TABLE t(x TEXT); INSERT INTO t VALUES ('a');")
            .unwrap();
        assert_eq!(mode(&path), 0o600);
        for side in ["store.db-wal", "store.db-shm"] {
            let p = dir.join(side);
            if p.exists() {
                assert_eq!(mode(&p), 0o600, "{side}");
            }
        }
    }
    // a store left readable by others, as stores made earlier were, is made owner-only when it is opened
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let v = Vault::open(&path, &k).unwrap();
    v.connection()
        .execute_batch("INSERT INTO t VALUES ('b');")
        .unwrap();
    assert_eq!(mode(&path), 0o600);
    let _ = std::fs::remove_dir_all(&dir);
}
