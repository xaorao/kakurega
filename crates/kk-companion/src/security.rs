use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

pub const PAIRING_TTL: Duration = Duration::from_secs(300);
pub const SESSION_TTL: Duration = Duration::from_secs(8 * 3600);
pub const MAX_FAILED_PAIRINGS: u32 = 5;
pub const LOCKOUT: Duration = Duration::from_secs(60);

pub fn random_token(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("operating system randomness");
    kk_evidence::b64::encode(&b)
}

fn hash(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub csrf: String,
}

struct Session {
    csrf: String,
    expires: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PairError {
    Locked,
    Invalid,
}

pub struct Security {
    pairing_ttl: Duration,
    pairing: Mutex<HashMap<[u8; 32], Instant>>,
    sessions: Mutex<HashMap<[u8; 32], Session>>,
    failures: Mutex<(u32, Option<Instant>)>,
    admin_token: String,
}

impl Security {
    pub fn new() -> Self {
        Security::with_pairing_ttl(PAIRING_TTL)
    }

    pub fn with_pairing_ttl(ttl: Duration) -> Self {
        Security {
            pairing_ttl: ttl,
            pairing: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new((0, None)),
            admin_token: random_token(32),
        }
    }

    pub fn admin_token(&self) -> &str {
        &self.admin_token
    }

    pub fn issue_pairing_code(&self) -> String {
        let code = random_token(16);
        let mut p = self.pairing.lock().expect("pairing lock");
        p.retain(|_, exp| *exp > Instant::now());
        p.insert(hash(&code), Instant::now() + self.pairing_ttl);
        code
    }

    /// Exchanges a one-time pairing code for a session.
    ///
    /// The code is checked first. A code is 128 random bits, so guessing one is hopeless however fast it is tried; what the
    /// throttle protects is the log and the CPU, not the code. If a lockout also refused the right code, any local program
    /// could keep the owner out of their own app by sending wrong codes, so only wrong guesses are ever throttled.
    pub fn pair(&self, code: &str) -> Result<(String, SessionInfo), PairError> {
        let h = hash(code);
        let ok = {
            let mut p = self.pairing.lock().expect("pairing lock");
            match p.remove(&h) {
                Some(exp) => exp > Instant::now(),
                None => false,
            }
        };
        if !ok {
            let mut f = self.failures.lock().expect("failure lock");
            if let Some(until) = f.1 {
                if Instant::now() < until {
                    return Err(PairError::Locked);
                }
                *f = (0, None);
            }
            f.0 += 1;
            if f.0 >= MAX_FAILED_PAIRINGS {
                f.1 = Some(Instant::now() + LOCKOUT);
            }
            return Err(PairError::Invalid);
        }
        *self.failures.lock().expect("failure lock") = (0, None);
        let token = random_token(32);
        let csrf = random_token(32);
        self.sessions.lock().expect("session lock").insert(
            hash(&token),
            Session {
                csrf: csrf.clone(),
                expires: Instant::now() + SESSION_TTL,
            },
        );
        Ok((token, SessionInfo { csrf }))
    }

    pub fn session(&self, token: &str) -> Option<SessionInfo> {
        let mut s = self.sessions.lock().expect("session lock");
        let h = hash(token);
        match s.get(&h) {
            Some(sess) if sess.expires > Instant::now() => Some(SessionInfo {
                csrf: sess.csrf.clone(),
            }),
            Some(_) => {
                s.remove(&h);
                None
            }
            None => None,
        }
    }

    pub fn revoke(&self, token: &str) {
        self.sessions
            .lock()
            .expect("session lock")
            .remove(&hash(token));
    }

    pub fn revoke_all(&self) {
        self.sessions.lock().expect("session lock").clear();
    }

    pub fn active_sessions(&self) -> usize {
        self.sessions.lock().expect("session lock").len()
    }

    pub fn csrf_matches(&self, token: &str, presented: &str) -> bool {
        self.session(token)
            .is_some_and(|s| s.csrf.as_bytes().ct_eq(presented.as_bytes()).into())
    }

    pub fn admin_matches(&self, presented: &str) -> bool {
        self.admin_token
            .as_bytes()
            .ct_eq(presented.as_bytes())
            .into()
    }
}

impl Default for Security {
    fn default() -> Self {
        Security::new()
    }
}
