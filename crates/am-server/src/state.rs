//! The server's state (AD-13): the port, the allowed origins, the injected
//! clock, the open project's store path, the event log, the CLI token and
//! the in-memory sign-in table (launch codes, sessions, the pairing code).
//! Nothing here is ever written to disk, a URL or a log, except the launch
//! code in the opened URL's fragment and the CLI token in its own file
//! (written by `agentic serve`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use am_domain::Clock;
use axum::http::{HeaderMap, Uri, header};
use jiff::{SignedDuration, Timestamp};

use crate::events::EventLog;
use crate::pairing::{Pairing, PairingCode, PairingLocked};
use crate::secret::{CliToken, ct_eq, random_hex};
use crate::{EVENT_WINDOW, LAUNCH_CODE_TTL};

/// The real clock, when none is injected.
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Whether `then` is no later than `now` and no more than `ttl` before it.
/// A time in the future (the clock stepped back since) is not within.
pub(crate) fn within(now: Timestamp, then: Timestamp, ttl: Duration) -> bool {
    let ttl = SignedDuration::from_secs(i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX));
    let age = now.duration_since(then);
    age >= SignedDuration::ZERO && age <= ttl
}

/// The sign-in table, behind one lock.
#[derive(Default)]
pub(crate) struct Auth {
    /// Outstanding launch codes and when each was issued.
    launch_codes: Vec<(String, Timestamp)>,
    /// Open sessions: (session id, CSRF value).
    sessions: Vec<(String, String)>,
    pub(crate) pairing: Pairing,
}

struct Shared {
    clock: Arc<dyn Clock>,
    project_db: Option<PathBuf>,
    events: EventLog,
    cli_token: CliToken,
    auth: Mutex<Auth>,
}

/// The server's state; clones share it (the allowed origins excepted,
/// which are fixed before serving).
#[derive(Clone)]
pub struct AppState {
    port: u16,
    extra_origins: Vec<String>,
    shared: Arc<Shared>,
}

/// Builds an [`AppState`]: the clock (system by default), the event window
/// (10 000 by default), the open project's `state.db` (none by default)
/// and extra allowed origins.
pub struct AppStateBuilder {
    port: u16,
    clock: Option<Arc<dyn Clock>>,
    event_window: usize,
    project_db: Option<PathBuf>,
    extra_origins: Vec<String>,
}

impl AppStateBuilder {
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// How many events the log holds for `since=` replay.
    pub fn event_window(mut self, n: usize) -> Self {
        self.event_window = n;
        self
    }

    /// The open project's `state.db`, read only through `am_store::Reader`.
    pub fn project_db(mut self, path: PathBuf) -> Self {
        self.project_db = Some(path);
        self
    }

    /// Allow another origin (the Tauri origin in desktop builds).
    pub fn allow_origin(mut self, origin: &str) -> Self {
        self.extra_origins.push(origin.to_string());
        self
    }

    pub fn build(self) -> AppState {
        AppState {
            port: self.port,
            extra_origins: self.extra_origins,
            shared: Arc::new(Shared {
                clock: self.clock.unwrap_or_else(|| Arc::new(SystemClock)),
                project_db: self.project_db,
                events: EventLog::new(self.event_window),
                cli_token: CliToken::generate(),
                auth: Mutex::new(Auth::default()),
            }),
        }
    }
}

impl AppState {
    /// The system clock, the default event window, no project.
    pub fn new(port: u16) -> Self {
        AppState::builder(port).build()
    }

    pub fn builder(port: u16) -> AppStateBuilder {
        AppStateBuilder {
            port,
            clock: None,
            event_window: EVENT_WINDOW,
            project_db: None,
            extra_origins: Vec::new(),
        }
    }

    /// Allow another origin (the Tauri origin in desktop builds).
    pub fn allow_origin(mut self, origin: &str) -> Self {
        self.extra_origins.push(origin.to_string());
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The session cookie's name, per port, so another localhost service
    /// never receives a cookie it could replay here.
    pub fn cookie_name(&self) -> String {
        format!("am_s_{}", self.port)
    }

    /// Issues a single-use launch code valid for 60 s.
    pub fn issue_launch_code(&self) -> String {
        let code = random_hex(32);
        let now = self.now();
        self.auth().launch_codes.push((code.clone(), now));
        code
    }

    /// The URL the browser opens: the code travels only in the fragment.
    pub fn launch_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/#code={}",
            self.port,
            self.issue_launch_code()
        )
    }

    /// This launch's CLI token.
    pub fn cli_token(&self) -> CliToken {
        self.shared.cli_token.clone()
    }

    /// This launch's event log.
    pub fn events(&self) -> EventLog {
        self.shared.events.clone()
    }

    /// Issues a new pairing code (8 base32 characters, single use, valid
    /// 60 s), invalidating any unused one; refused while pairing is locked.
    /// Only the `agentic serve` terminal calls this: no route reaches it.
    pub fn issue_pairing_code(&self) -> Result<PairingCode, PairingLocked> {
        let now = self.now();
        self.auth().pairing.issue(now)
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.shared.clock.now()
    }

    pub(crate) fn project_db(&self) -> Option<PathBuf> {
        self.shared.project_db.clone()
    }

    pub(crate) fn auth(&self) -> MutexGuard<'_, Auth> {
        self.shared
            .auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The Host is exactly `127.0.0.1:<port>` or `localhost:<port>` (one
    /// Host header), and an absolute-form request target, when used, names
    /// one of the two as well (over `http`).
    pub(crate) fn host_ok(&self, headers: &HeaderMap, uri: &Uri) -> bool {
        let mut hosts = headers.get_all(header::HOST).iter();
        let (Some(host), None) = (hosts.next(), hosts.next()) else {
            return false;
        };
        let Ok(host) = host.to_str() else {
            return false;
        };
        if !self.is_own_host(host) {
            return false;
        }
        match uri.authority() {
            None => uri.scheme().is_none(),
            Some(authority) => {
                uri.scheme_str() == Some("http") && self.is_own_host(authority.as_str())
            }
        }
    }

    fn is_own_host(&self, host: &str) -> bool {
        host == format!("127.0.0.1:{}", self.port) || host == format!("localhost:{}", self.port)
    }

    pub(crate) fn origin_allowed(&self, origin: &str) -> bool {
        origin == format!("http://127.0.0.1:{}", self.port)
            || origin == format!("http://localhost:{}", self.port)
            || self.extra_origins.iter().any(|o| o == origin)
    }

    pub(crate) fn token_ok(&self, presented: &str) -> bool {
        self.shared.cli_token.matches(presented)
    }

    /// The CSRF value of the session the request's `am_s_<port>` cookie
    /// names, if any.
    pub(crate) fn session_csrf(&self, headers: &HeaderMap) -> Option<String> {
        let cookie_name = self.cookie_name();
        let sids: Vec<String> = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|kv| kv.trim().split_once('='))
            .filter(|(k, _)| *k == cookie_name)
            .map(|(_, v)| v.trim().to_string())
            .collect();
        let auth = self.auth();
        for sid in &sids {
            let mut found = None;
            for (id, csrf) in &auth.sessions {
                if ct_eq(id, sid) {
                    found = Some(csrf.clone());
                }
            }
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Consumes a launch code: `true` when it was outstanding and fresh. A
    /// failed exchange invalidates every outstanding code (AD-13).
    pub(crate) fn take_launch_code(&self, code: &str) -> bool {
        let now = self.now();
        let mut auth = self.auth();
        let mut hit = None;
        for (i, (c, _)) in auth.launch_codes.iter().enumerate() {
            if ct_eq(c, code) {
                hit = Some(i);
            }
        }
        let valid = match hit {
            Some(i) => {
                let (_, issued) = auth.launch_codes.remove(i);
                within(now, issued, LAUNCH_CODE_TTL)
            }
            None => false,
        };
        if !valid {
            auth.launch_codes.clear();
        }
        valid
    }

    /// Opens a session: (session id, CSRF value), 256 bits each.
    pub(crate) fn open_session(&self) -> (String, String) {
        let sid = random_hex(32);
        let csrf = random_hex(32);
        self.auth().sessions.push((sid.clone(), csrf.clone()));
        (sid, csrf)
    }
}
