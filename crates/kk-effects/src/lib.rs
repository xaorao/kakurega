use kk_vault::rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use kk_vault::Vault;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    E1,
    E2,
    E3,
    E4,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Class::E1 => "E1",
            Class::E2 => "E2",
            Class::E3 => "E3",
            Class::E4 => "E4",
        }
    }

    fn parse(s: &str) -> Option<Class> {
        match s {
            "E1" => Some(Class::E1),
            "E2" => Some(Class::E2),
            "E3" => Some(Class::E3),
            "E4" => Some(Class::E4),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    FileWrite,
    Issue,
    Webhook,
    Email,
    CommitPush,
    RemoteApi,
    Funds,
    PermissionChange,
    Release,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::FileWrite => "FILE_WRITE",
            Kind::Issue => "ISSUE",
            Kind::Webhook => "WEBHOOK",
            Kind::Email => "EMAIL",
            Kind::CommitPush => "COMMIT_PUSH",
            Kind::RemoteApi => "REMOTE_API",
            Kind::Funds => "FUNDS",
            Kind::PermissionChange => "PERMISSION_CHANGE",
            Kind::Release => "RELEASE",
        }
    }

    fn needs_specific_grant(self) -> bool {
        matches!(self, Kind::Funds | Kind::PermissionChange)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Intended,
    Authorized,
    Attempted,
    Confirmed,
    Failed,
    Uncertain,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Intended => "INTENDED",
            State::Authorized => "AUTHORIZED",
            State::Attempted => "ATTEMPTED",
            State::Confirmed => "CONFIRMED",
            State::Failed => "FAILED",
            State::Uncertain => "UNCERTAIN",
        }
    }

    fn parse(s: &str) -> Option<State> {
        Some(match s {
            "INTENDED" => State::Intended,
            "AUTHORIZED" => State::Authorized,
            "ATTEMPTED" => State::Attempted,
            "CONFIRMED" => State::Confirmed,
            "FAILED" => State::Failed,
            "UNCERTAIN" => State::Uncertain,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, State::Confirmed | State::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    pub specific_to: Option<Kind>,
    pub exact_amount: Option<String>,
    pub recipient: Option<String>,
    pub purpose: Option<String>,
}

impl Grant {
    pub fn general(id: &str) -> Grant {
        Grant {
            id: id.to_string(),
            specific_to: None,
            exact_amount: None,
            recipient: None,
            purpose: None,
        }
    }

    pub fn specific(id: &str, kind: Kind) -> Grant {
        Grant {
            specific_to: Some(kind),
            ..Grant::general(id)
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewIntent {
    pub intent_id: String,
    pub kind: Kind,
    pub class: Class,
    pub idempotency_key: Option<String>,
    pub key_window_secs: Option<i64>,
    pub cooldown_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub intent_id: String,
    pub kind: String,
    pub class: Class,
    pub state: State,
    pub attempts: i64,
    pub grant_ref: Option<String>,
    pub idempotency_key: Option<String>,
    pub cooldown_until: Option<i64>,
}

#[derive(Debug)]
pub enum LedgerError {
    NotFound,
    AlreadyExists,
    IllegalTransition { from: State, to: State },
    GrantInsufficient(&'static str),
    AlreadyAuthorized,
    KeyWindowExpired,
    NoAutomaticRetry,
    CooldownActive,
    Remote(String),
    Storage(String),
    OwnedElsewhere,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::NotFound => f.write_str("unknown intent"),
            LedgerError::AlreadyExists => f.write_str("intent already exists"),
            LedgerError::IllegalTransition { from, to } => {
                write!(f, "illegal transition {} -> {}", from.as_str(), to.as_str())
            }
            LedgerError::GrantInsufficient(why) => write!(f, "grant insufficient: {why}"),
            LedgerError::AlreadyAuthorized => f.write_str("intent already authorised"),
            LedgerError::KeyWindowExpired => {
                f.write_str("idempotency key retention window has expired")
            }
            LedgerError::NoAutomaticRetry => {
                f.write_str("outcome unknown; automatic retry is not allowed")
            }
            LedgerError::CooldownActive => f.write_str("cool-down has not elapsed"),
            LedgerError::Remote(m) => write!(f, "remote error: {m}"),
            LedgerError::Storage(m) => write!(f, "storage error: {m}"),
            LedgerError::OwnedElsewhere => f.write_str("another process owns this ledger"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<kk_vault::rusqlite::Error> for LedgerError {
    fn from(e: kk_vault::rusqlite::Error) -> Self {
        LedgerError::Storage(e.to_string())
    }
}

pub trait Clock {
    fn now(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    Delivered,
    Rejected(String),
    OutcomeUnknown,
}

pub trait Remote {
    fn send(&self, idempotency_key: Option<&str>, marker: &str, payload: &[u8]) -> SendResult;
    fn read_back(&self, marker: &str) -> Result<bool, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    ConfirmedByUser,
    FailedByUser,
}

pub struct Ledger<C: Clock = SystemClock> {
    vault: Vault,
    clock: C,
    _owner_lock: Option<std::fs::File>,
}

#[cfg(feature = "failpoints")]
pub fn failpoint(name: &str) {
    if std::env::var("KK_FAILPOINT")
        .map(|v| v == name)
        .unwrap_or(false)
    {
        std::process::abort();
    }
}

#[cfg(not(feature = "failpoints"))]
#[inline(always)]
pub fn failpoint(_name: &str) {}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS effects(
  intent_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  class TEXT NOT NULL,
  state TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  grant_ref TEXT,
  idempotency_key TEXT,
  key_window_secs INTEGER,
  first_attempt_at INTEGER,
  cooldown_secs INTEGER NOT NULL DEFAULT 0,
  cooldown_until INTEGER,
  readback_miss INTEGER NOT NULL DEFAULT 0,
  note TEXT
);
CREATE TABLE IF NOT EXISTS effect_log(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  intent_id TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  at INTEGER NOT NULL,
  detail TEXT
);
CREATE TRIGGER IF NOT EXISTS effect_log_no_update BEFORE UPDATE ON effect_log
BEGIN SELECT RAISE(ABORT, 'effect_log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS effect_log_no_delete BEFORE DELETE ON effect_log
BEGIN SELECT RAISE(ABORT, 'effect_log is append-only'); END;
";

impl<C: Clock> Ledger<C> {
    pub fn open_owner(
        vault: Vault,
        clock: C,
        lock_path: &std::path::Path,
    ) -> Result<Self, LedgerError> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .map_err(|e| LedgerError::Storage(e.to_string()))?;
        lock.try_lock().map_err(|_| LedgerError::OwnedElsewhere)?;
        vault.connection().execute_batch(SCHEMA)?;
        let mut ledger = Ledger {
            vault,
            clock,
            _owner_lock: Some(lock),
        };
        ledger.recover()?;
        Ok(ledger)
    }

    pub fn open_shared(vault: Vault, clock: C) -> Result<Self, LedgerError> {
        vault.connection().execute_batch(SCHEMA)?;
        Ok(Ledger {
            vault,
            clock,
            _owner_lock: None,
        })
    }

    fn recover(&mut self) -> Result<(), LedgerError> {
        let now = self.clock.now();
        let tx = self
            .vault
            .connection()
            .unchecked_transaction()
            .map_err(LedgerError::from)?;
        let stuck: Vec<String> = {
            let mut stmt = tx.prepare("SELECT intent_id FROM effects WHERE state = 'ATTEMPTED'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in stuck {
            tx.execute(
                "UPDATE effects SET state = 'UNCERTAIN' WHERE intent_id = ?1 AND state = 'ATTEMPTED'",
                [&id],
            )?;
            tx.execute(
                "INSERT INTO effect_log(intent_id, from_state, to_state, at, detail) VALUES (?1, 'ATTEMPTED', 'UNCERTAIN', ?2, 'crash recovery')",
                params![id, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn into_vault(self) -> Vault {
        self.vault
    }

    pub fn get(&self, intent_id: &str) -> Result<Record, LedgerError> {
        self.vault
            .connection()
            .query_row(
                "SELECT intent_id, kind, class, state, attempts, grant_ref, idempotency_key, cooldown_until FROM effects WHERE intent_id = ?1",
                [intent_id],
                |r| {
                    Ok(Record {
                        intent_id: r.get(0)?,
                        kind: r.get(1)?,
                        class: Class::parse(&r.get::<_, String>(2)?).unwrap_or(Class::E3),
                        state: State::parse(&r.get::<_, String>(3)?).unwrap_or(State::Uncertain),
                        attempts: r.get(4)?,
                        grant_ref: r.get(5)?,
                        idempotency_key: r.get(6)?,
                        cooldown_until: r.get(7)?,
                    })
                },
            )
            .optional()?
            .ok_or(LedgerError::NotFound)
    }

    pub fn log_len(&self) -> Result<i64, LedgerError> {
        Ok(self
            .vault
            .connection()
            .query_row("SELECT count(*) FROM effect_log", [], |r| r.get(0))?)
    }

    pub fn record_intent(&mut self, intent: &NewIntent) -> Result<(), LedgerError> {
        if matches!(intent.class, Class::E1) && intent.idempotency_key.is_none() {
            return Err(LedgerError::GrantInsufficient(
                "E1 effects need an idempotency key",
            ));
        }
        let now = self.clock.now();
        let conn = self.vault.connection();
        let tx = conn.unchecked_transaction()?;
        let exists: bool = tx.query_row(
            "SELECT count(*) > 0 FROM effects WHERE intent_id = ?1",
            [&intent.intent_id],
            |r| r.get(0),
        )?;
        if exists {
            return Err(LedgerError::AlreadyExists);
        }
        tx.execute(
            "INSERT INTO effects(intent_id, kind, class, state, idempotency_key, key_window_secs, cooldown_secs) VALUES (?1, ?2, ?3, 'INTENDED', ?4, ?5, ?6)",
            params![
                intent.intent_id,
                intent.kind.as_str(),
                intent.class.as_str(),
                intent.idempotency_key,
                intent.key_window_secs,
                intent.cooldown_secs
            ],
        )?;
        tx.execute(
            "INSERT INTO effect_log(intent_id, from_state, to_state, at, detail) VALUES (?1, NULL, 'INTENDED', ?2, NULL)",
            params![intent.intent_id, now],
        )?;
        tx.commit()?;
        failpoint("after_intended");
        Ok(())
    }

    pub fn authorize(
        &mut self,
        intent_id: &str,
        kind: Kind,
        grant: &Grant,
    ) -> Result<(), LedgerError> {
        let rec = self.get(intent_id)?;
        if kind.needs_specific_grant() && grant.specific_to != Some(kind) {
            return Err(LedgerError::GrantInsufficient(
                "spending and permission changes need a grant naming that kind",
            ));
        }
        if rec.class == Class::E4 {
            let complete = grant.exact_amount.is_some()
                && grant.recipient.is_some()
                && grant.purpose.is_some();
            if !complete {
                return Err(LedgerError::GrantInsufficient(
                    "E4 effects need an exact amount, recipient and purpose",
                ));
            }
        }
        let now = self.clock.now();
        let tx = self.vault.connection().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE effects SET state = 'AUTHORIZED', grant_ref = ?2 WHERE intent_id = ?1 AND state = 'INTENDED'",
            params![intent_id, grant.id],
        )?;
        if changed != 1 {
            return Err(if rec.state == State::Authorized {
                LedgerError::AlreadyAuthorized
            } else {
                LedgerError::IllegalTransition {
                    from: rec.state,
                    to: State::Authorized,
                }
            });
        }
        tx.execute(
            "INSERT INTO effect_log(intent_id, from_state, to_state, at, detail) VALUES (?1, 'INTENDED', 'AUTHORIZED', ?2, ?3)",
            params![intent_id, now, grant.id],
        )?;
        tx.commit()?;
        failpoint("after_authorized");
        Ok(())
    }

    fn begin_attempt(
        &mut self,
        intent_id: &str,
        from: State,
        detail: &str,
    ) -> Result<(), LedgerError> {
        let now = self.clock.now();
        let tx =
            Transaction::new_unchecked(self.vault.connection(), TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE effects SET state = 'ATTEMPTED', attempts = attempts + 1, first_attempt_at = COALESCE(first_attempt_at, ?3) WHERE intent_id = ?1 AND state = ?2",
            params![intent_id, from.as_str(), now],
        )?;
        if changed != 1 {
            return Err(LedgerError::IllegalTransition {
                from,
                to: State::Attempted,
            });
        }
        tx.execute(
            "INSERT INTO effect_log(intent_id, from_state, to_state, at, detail) VALUES (?1, ?2, 'ATTEMPTED', ?3, ?4)",
            params![intent_id, from.as_str(), now, detail],
        )?;
        tx.commit()?;
        failpoint("after_attempted");
        Ok(())
    }

    fn finish(&mut self, intent_id: &str, to: State, detail: &str) -> Result<(), LedgerError> {
        let now = self.clock.now();
        let tx =
            Transaction::new_unchecked(self.vault.connection(), TransactionBehavior::Immediate)?;
        let (state_s, cooldown_secs): (String, i64) = tx.query_row(
            "SELECT state, cooldown_secs FROM effects WHERE intent_id = ?1",
            [intent_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let from = State::parse(&state_s).unwrap_or(State::Uncertain);
        if from.is_terminal() || !matches!(from, State::Attempted | State::Uncertain) {
            return Err(LedgerError::IllegalTransition { from, to });
        }
        let cooldown_until = if to == State::Uncertain && cooldown_secs > 0 {
            Some(now + cooldown_secs)
        } else {
            None
        };
        tx.execute(
            "UPDATE effects SET state = ?2, cooldown_until = COALESCE(?3, cooldown_until) WHERE intent_id = ?1",
            params![intent_id, to.as_str(), cooldown_until],
        )?;
        tx.execute(
            "INSERT INTO effect_log(intent_id, from_state, to_state, at, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![intent_id, from.as_str(), to.as_str(), now, detail],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn send_and_record(
        &mut self,
        intent_id: &str,
        remote: &dyn Remote,
        payload: &[u8],
    ) -> Result<State, LedgerError> {
        let rec = self.get(intent_id)?;
        let key = rec.idempotency_key.clone();
        let result = remote.send(key.as_deref(), intent_id, payload);
        failpoint("after_send");
        let outcome = match result {
            SendResult::Delivered => (State::Confirmed, "delivered"),
            SendResult::Rejected(_) => (State::Failed, "rejected by remote"),
            SendResult::OutcomeUnknown => (State::Uncertain, "outcome unknown"),
        };
        self.finish(intent_id, outcome.0, outcome.1)?;
        failpoint("after_outcome");
        Ok(outcome.0)
    }

    pub fn execute(
        &mut self,
        intent_id: &str,
        remote: &dyn Remote,
        payload: &[u8],
    ) -> Result<State, LedgerError> {
        let rec = self.get(intent_id)?;
        match rec.state {
            State::Confirmed | State::Failed => Ok(rec.state),
            State::Authorized => {
                self.begin_attempt(intent_id, State::Authorized, "first attempt")?;
                self.send_and_record(intent_id, remote, payload)
            }
            State::Intended => Err(LedgerError::GrantInsufficient("intent has no grant yet")),
            State::Attempted => Err(LedgerError::IllegalTransition {
                from: State::Attempted,
                to: State::Attempted,
            }),
            State::Uncertain => self.resume_uncertain(intent_id, remote, payload),
        }
    }

    fn resume_uncertain(
        &mut self,
        intent_id: &str,
        remote: &dyn Remote,
        payload: &[u8],
    ) -> Result<State, LedgerError> {
        let rec = self.get(intent_id)?;
        match rec.class {
            Class::E3 | Class::E4 => Err(LedgerError::NoAutomaticRetry),
            Class::E1 => {
                let (window, first): (Option<i64>, Option<i64>) = self
                    .vault
                    .connection()
                    .query_row(
                    "SELECT key_window_secs, first_attempt_at FROM effects WHERE intent_id = ?1",
                    [intent_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if let (Some(w), Some(f)) = (window, first) {
                    if self.clock.now() > f + w {
                        return Err(LedgerError::KeyWindowExpired);
                    }
                }
                self.begin_attempt(
                    intent_id,
                    State::Uncertain,
                    "idempotent retry with the same key",
                )?;
                self.send_and_record(intent_id, remote, payload)
            }
            Class::E2 => {
                let found = remote.read_back(intent_id).map_err(LedgerError::Remote)?;
                if found {
                    self.finish(intent_id, State::Confirmed, "read-back found the effect")?;
                    return Ok(State::Confirmed);
                }
                let misses: i64 = self.vault.connection().query_row(
                    "SELECT readback_miss FROM effects WHERE intent_id = ?1",
                    [intent_id],
                    |r| r.get(0),
                )?;
                if misses >= 1 {
                    return Err(LedgerError::NoAutomaticRetry);
                }
                self.vault.connection().execute(
                    "UPDATE effects SET readback_miss = readback_miss + 1 WHERE intent_id = ?1",
                    [intent_id],
                )?;
                self.begin_attempt(
                    intent_id,
                    State::Uncertain,
                    "single retry after read-back found nothing",
                )?;
                self.send_and_record(intent_id, remote, payload)
            }
        }
    }

    pub fn resolve_uncertain(
        &mut self,
        intent_id: &str,
        resolution: Resolution,
        note: &str,
    ) -> Result<(), LedgerError> {
        let rec = self.get(intent_id)?;
        if rec.state != State::Uncertain {
            return Err(LedgerError::IllegalTransition {
                from: rec.state,
                to: State::Confirmed,
            });
        }
        let to = match resolution {
            Resolution::ConfirmedByUser => State::Confirmed,
            Resolution::FailedByUser => State::Failed,
        };
        self.finish(intent_id, to, &format!("user resolution: {note}"))
    }

    pub fn allow_new_attempt_after(&self, previous: &str) -> Result<(), LedgerError> {
        let rec = self.get(previous)?;
        if rec.state == State::Uncertain {
            return Err(LedgerError::NoAutomaticRetry);
        }
        if let Some(until) = rec.cooldown_until {
            if self.clock.now() < until {
                return Err(LedgerError::CooldownActive);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-effects-unit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    fn new_ledger(dir: &std::path::Path) -> Ledger {
        let key = kk_vault::VaultKey::from_bytes([9u8; 32]);
        let vault = Vault::create(&dir.join("l.db"), &key).expect("vault");
        Ledger::open_owner(vault, SystemClock, &dir.join("l.lock")).expect("ledger")
    }

    #[test]
    fn a_late_completion_cannot_overwrite_a_terminal_state() {
        let dir = scratch("terminal");
        let mut l = new_ledger(&dir);
        l.record_intent(&NewIntent {
            intent_id: "a".into(),
            kind: Kind::Email,
            class: Class::E3,
            idempotency_key: None,
            key_window_secs: None,
            cooldown_secs: 0,
        })
        .expect("intent");
        l.authorize("a", Kind::Email, &Grant::general("g"))
            .expect("authorize");
        l.begin_attempt("a", State::Authorized, "t")
            .expect("attempt");
        l.finish("a", State::Confirmed, "done").expect("finish");
        for to in [State::Failed, State::Uncertain, State::Confirmed] {
            assert!(matches!(
                l.finish("a", to, "late"),
                Err(LedgerError::IllegalTransition { .. })
            ));
        }
        assert_eq!(l.get("a").expect("get").state, State::Confirmed);
    }

    #[test]
    fn a_second_owner_is_refused_so_recovery_cannot_clobber_a_live_attempt() {
        let dir = scratch("owner");
        let first = new_ledger(&dir);
        let key = kk_vault::VaultKey::from_bytes([9u8; 32]);
        let vault = Vault::open(&dir.join("l.db"), &key).expect("vault");
        assert!(matches!(
            Ledger::open_owner(vault, SystemClock, &dir.join("l.lock")),
            Err(LedgerError::OwnedElsewhere)
        ));
        drop(first);
        let vault = Vault::open(&dir.join("l.db"), &key).expect("vault");
        assert!(Ledger::open_owner(vault, SystemClock, &dir.join("l.lock")).is_ok());
    }
}
