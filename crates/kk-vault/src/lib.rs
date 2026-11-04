use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::Connection;
use std::fmt;
use std::path::Path;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use rusqlite;

pub const KEY_LEN: usize = 32;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey([u8; KEY_LEN]);

impl VaultKey {
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        VaultKey(bytes)
    }

    fn pragma_value(&self) -> String {
        let mut s = String::with_capacity(KEY_LEN * 2 + 3);
        s.push_str("x'");
        for b in &self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s.push('\'');
        s
    }
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultKey(<redacted>)")
    }
}

#[derive(Debug)]
pub enum VaultError {
    WrongKeyOrNotAVault,
    AlreadyExists,
    Missing,
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::WrongKeyOrNotAVault => f.write_str("wrong key, or the file is not a vault"),
            VaultError::AlreadyExists => f.write_str("a vault already exists at this path"),
            VaultError::Missing => f.write_str("no vault at this path"),
            VaultError::Sqlite(e) => write!(f, "storage error: {e}"),
            VaultError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<rusqlite::Error> for VaultError {
    fn from(e: rusqlite::Error) -> Self {
        VaultError::Sqlite(e)
    }
}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        VaultError::Io(e)
    }
}

pub struct Vault {
    conn: Connection,
}

fn configure(conn: &Connection, key: &VaultKey) -> Result<(), VaultError> {
    let mut pragma = format!("PRAGMA key = \"{}\";", key.pragma_value());
    let r = conn.execute_batch(&pragma);
    pragma.zeroize();
    r?;
    conn.execute_batch(
        "PRAGMA cipher_memory_security = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA secure_delete = ON;
         PRAGMA foreign_keys = ON;",
    )?;
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(_) => {}
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::NotADatabase =>
        {
            return Err(VaultError::WrongKeyOrNotAVault)
        }
        Err(e) => return Err(e.into()),
    }
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(VaultError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
    }
    conn.execute_batch("PRAGMA synchronous = FULL;")?;
    Ok(())
}

fn guard(ctx: AuthContext<'_>) -> Authorization {
    if let AuthAction::Pragma {
        pragma_name,
        pragma_value,
    } = ctx.action
    {
        let n = pragma_name.to_ascii_lowercase();
        let sets = pragma_value.is_some();
        let forbidden_always = matches!(n.as_str(), "key" | "rekey" | "hexkey" | "hexrekey");
        let forbidden_when_set = matches!(
            n.as_str(),
            "temp_store"
                | "temp_store_directory"
                | "cipher_memory_security"
                | "journal_mode"
                | "secure_delete"
                | "synchronous"
        );
        if forbidden_always || (sets && forbidden_when_set) {
            return Authorization::Deny;
        }
    }
    if let AuthAction::Attach { .. } = ctx.action {
        return Authorization::Deny;
    }
    Authorization::Allow
}

fn install_guard(conn: &mut Connection) -> Result<(), VaultError> {
    Ok(conn.authorizer(Some(guard))?)
}

fn lift_guard(conn: &mut Connection) -> Result<(), VaultError> {
    Ok(conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?)
}

impl Vault {
    pub fn create(path: &Path, key: &VaultKey) -> Result<Vault, VaultError> {
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        let mut conn = Connection::open(path)?;
        configure(&conn, key)?;
        install_guard(&mut conn)?;
        Ok(Vault { conn })
    }

    pub fn open(path: &Path, key: &VaultKey) -> Result<Vault, VaultError> {
        if !path.exists() {
            return Err(VaultError::Missing);
        }
        let mut conn = Connection::open(path)?;
        configure(&conn, key)?;
        install_guard(&mut conn)?;
        Ok(Vault { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn cipher_version(&self) -> Result<String, VaultError> {
        Ok(self
            .conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))?)
    }

    pub fn rekey(&mut self, new_key: &VaultKey) -> Result<(), VaultError> {
        let mut pragma = format!("PRAGMA rekey = \"{}\";", new_key.pragma_value());
        lift_guard(&mut self.conn)?;
        let r = self.conn.execute_batch(&pragma);
        let relock = install_guard(&mut self.conn);
        pragma.zeroize();
        relock?;
        Ok(r?)
    }

    pub fn backup_to(&mut self, dest: &Path) -> Result<(), VaultError> {
        if dest.exists() {
            return Err(VaultError::AlreadyExists);
        }
        let d = dest
            .to_str()
            .ok_or(VaultError::Missing)?
            .replace('\'', "''");
        lift_guard(&mut self.conn)?;
        let r = self.conn.execute_batch(&format!("VACUUM INTO '{d}'"));
        let relock = install_guard(&mut self.conn);
        relock?;
        Ok(r?)
    }
}
