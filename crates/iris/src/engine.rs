//! The engine: the accounts, the tokens and the sync schedule
//! (specification 1, sections 4.6, 4.7 and 4.9).

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iris_auth::{AccessToken, Client, Secret};
use iris_gmail::Gmail;
use iris_store::Store;
use serde::{Deserialize, Serialize};

/// `account.state` of an account that syncs.
pub const STATE_OK: &str = "ok";
/// `account.state` when Google refused the stored sign-in. The person must
/// run `iris account add` again (specification 1, section 4.7).
pub const STATE_NEEDS_SIGN_IN: &str = "needs_sign_in";

/// The time between two incremental syncs (specification 1, section 4.6).
pub const SYNC_EVERY: Duration = Duration::from_secs(60);
/// The time between two runs of the content cleanup (section 4.5).
pub const CLEAN_EVERY: Duration = Duration::from_secs(3600);

/// Get a new access token when the old one has less time than this.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub enum Error {
    Auth(iris_auth::Error),
    Gmail(iris_gmail::Error),
    Store(iris_store::Error),
    Sync(iris_sync::Error),
    /// The account has no stored sign-in, or Google refused it.
    NeedsSignIn,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Auth(e) => write!(f, "sign-in: {e}"),
            Error::Gmail(e) => write!(f, "Gmail: {e}"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Sync(e) => write!(f, "sync: {e}"),
            Error::NeedsSignIn => {
                f.write_str("the account needs a new sign-in: run `iris account add <client.json>`")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_auth::Error> for Error {
    fn from(e: iris_auth::Error) -> Self {
        Error::Auth(e)
    }
}
impl From<iris_gmail::Error> for Error {
    fn from(e: iris_gmail::Error) -> Self {
        Error::Gmail(e)
    }
}
impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}
impl From<iris_sync::Error> for Error {
    fn from(e: iris_sync::Error) -> Self {
        Error::Sync(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The sign-in of one account, sealed in `account.token`: the OAuth client
/// file and the refresh token that Google gave to that client. One sealed
/// value keeps the two together, so each account can have its own client,
/// and a backup (one database snapshot) has a matching pair.
#[derive(Serialize, Deserialize)]
struct Grant {
    client: Box<serde_json::value::RawValue>,
    refresh: String,
}

/// Save a completed sign-in: find the address of the mailbox, add the
/// account (or find the account with this address), and store the client
/// file and the refresh token sealed, in one write. Gives the account ID and
/// the address.
pub fn save_sign_in(
    store: &Store,
    gmail: &Gmail,
    client_json: &[u8],
    tokens: &iris_auth::Tokens,
) -> Result<(i64, String)> {
    // The caller read the client with `Client::from_json`, so the file is JSON.
    let client = serde_json::from_slice::<Box<serde_json::value::RawValue>>(client_json)
        .map_err(|_| iris_auth::Error::ClientFile("the file is not an OAuth client JSON file"))?;
    let grant = Grant {
        client,
        refresh: tokens.refresh.expose().to_string(),
    };
    let plain = serde_json::to_vec(&grant).expect("a grant serializes");
    let sealed = store.seal(&plain)?;
    let profile = gmail.profile(tokens.access.token.expose())?;
    let address = profile.email_address;
    let id = store.add_account("gmail", &address)?;
    store.write(|tx| {
        tx.execute(
            "UPDATE account SET token = ?2, state = ?3 WHERE id = ?1",
            (id, &sealed, STATE_OK),
        )?;
        Ok(())
    })?;
    Ok((id, address))
}

/// Read the sealed sign-in of `account`: the client, the refresh token, and
/// the sealed value itself.
fn load_grant(store: &Store, account: i64) -> Result<(Client, Secret, Vec<u8>)> {
    let sealed: Option<Vec<u8>> = store.read(|c| {
        Ok(
            c.query_row("SELECT token FROM account WHERE id = ?1", [account], |r| {
                r.get(0)
            })?,
        )
    })?;
    let sealed = sealed.ok_or(Error::NeedsSignIn)?;
    let grant: Grant = serde_json::from_slice(&store.unseal(&sealed)?)
        .map_err(|_| iris_store::Error::Key("the stored sign-in is not valid"))?;
    let client = Client::from_json(grant.client.get().as_bytes())?;
    Ok((client, Secret::new(grant.refresh), sealed))
}

/// Set `account` to "needs sign-in", but only while `account.token` is still
/// `sealed`: the sign-in that failed. A new sign-in of `iris account add`
/// in the meantime keeps its state. Gives true if the state changed.
pub fn mark_needs_sign_in(store: &Store, account: i64, sealed: &[u8]) -> Result<bool> {
    Ok(store.write(|tx| {
        Ok(tx.execute(
            "UPDATE account SET state = ?2 WHERE id = ?1 AND token = ?3",
            (account, STATE_NEEDS_SIGN_IN, sealed),
        )? > 0)
    })?)
}

/// Each account with a stored sign-in, with its state.
pub fn signed_in_accounts(store: &Store) -> Result<Vec<(i64, Option<String>)>> {
    Ok(store.read(|c| {
        let mut st =
            c.prepare("SELECT id, state FROM account WHERE token IS NOT NULL ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?)
}

/// The accounts that the engine syncs: each account with a stored sign-in
/// that Google did not refuse.
pub fn sync_accounts(store: &Store) -> Result<Vec<i64>> {
    Ok(store.read(|c| {
        let mut st = c.prepare(
            "SELECT id FROM account WHERE token IS NOT NULL
             AND (state IS NULL OR state != ?1) ORDER BY id",
        )?;
        let ids = st
            .query_map([STATE_NEEDS_SIGN_IN], |r| r.get(0))?
            .collect::<std::result::Result<Vec<i64>, _>>()?;
        Ok(ids)
    })?)
}

/// The sync of one account. The access token stays in memory only.
pub struct Worker {
    store: Arc<Store>,
    gmail: Gmail,
    account: i64,
    test_token_uri: Option<String>,
    token: Option<AccessToken>,
    /// The sealed sign-in that gave `token`.
    sealed: Vec<u8>,
}

impl Worker {
    /// `test_token_uri` is for tests: a token endpoint on 127.0.0.1.
    pub fn new(
        store: Arc<Store>,
        gmail: Gmail,
        account: i64,
        test_token_uri: Option<String>,
    ) -> Worker {
        Worker {
            store,
            gmail,
            account,
            test_token_uri,
            token: None,
            sealed: Vec::new(),
        }
    }

    /// Sync the account one time (specification 1, section 4.3).
    pub fn sync(&mut self) -> Result<iris_sync::Synced> {
        self.call(iris_sync::sync)
    }

    /// Verify the mirror of the account one time (specification 1, section 4.8).
    pub fn verify(&mut self) -> Result<iris_sync::Report> {
        self.call(iris_sync::verify)
    }

    /// Call `f` with an access token. On HTTP 401, get a new access token one
    /// time and call again. If the new token gets HTTP 401 too, the account
    /// needs a new sign-in (specification 1, section 4.7).
    fn call<T>(
        &mut self,
        f: impl Fn(&Store, &Gmail, i64, &str) -> iris_sync::Result<T>,
    ) -> Result<T> {
        for _ in 0..2 {
            let token = self.access()?;
            match f(&self.store, &self.gmail, self.account, token.expose()) {
                Err(iris_sync::Error::Gmail(iris_gmail::Error::Unauthorized)) => {
                    self.token = None;
                }
                r => return Ok(r?),
            }
        }
        mark_needs_sign_in(&self.store, self.account, &self.sealed)?;
        Err(Error::NeedsSignIn)
    }

    /// A valid access token: the token in memory while the stored sign-in
    /// is the one that gave it, else a new one from the stored sign-in.
    fn access(&mut self) -> Result<Secret> {
        let (mut client, refresh, sealed) = load_grant(&self.store, self.account)?;
        // A new sign-in (`iris account add`) replaces the token in memory too.
        if let Some(t) = &self.token {
            if sealed == self.sealed && t.expires_at > Instant::now() + TOKEN_MARGIN {
                return Ok(t.token.clone());
            }
        }
        if let Some(uri) = &self.test_token_uri {
            client = client.with_test_token_uri(uri)?;
        }
        match client.refresh(&refresh) {
            Ok(t) => {
                let token = t.token.clone();
                self.token = Some(t);
                self.sealed = sealed;
                Ok(token)
            }
            // Google refused the refresh token: only a new sign-in helps. A
            // network or server error keeps the account, and the next turn
            // tries again (specification 1, section 4.7).
            Err(iris_auth::Error::Revoked) => {
                mark_needs_sign_in(&self.store, self.account, &sealed)?;
                Err(Error::NeedsSignIn)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Sync each [`SYNC_EVERY`] until the account leaves the list of
    /// [`sync_accounts`]. Each error goes to standard error.
    pub fn run(mut self) {
        loop {
            match self.sync() {
                Ok(_) => {}
                // Another process syncs the account: not an error.
                Err(Error::Sync(iris_sync::Error::OtherSync)) => {}
                Err(e) => say!("iris: account {}: {e}", self.account),
            }
            match sync_accounts(&self.store) {
                Ok(a) if !a.contains(&self.account) => return,
                _ => {}
            }
            std::thread::sleep(SYNC_EVERY);
        }
    }
}

/// The time between two tries of a run that another run blocks.
pub const BUSY_RETRY: Duration = Duration::from_secs(1);

/// The time between two turns of the understanding thread.
pub const UNDERSTAND_EVERY: Duration = Duration::from_secs(10);

/// The understanding thread (R1-28): the run after a sync
/// ([`crate::cost::after_sync`]) each [`UNDERSTAND_EVERY`], apart from the
/// sync threads, so a long run does not stop a sync. It gives the words and
/// the senders that search finds (spec 1 section 5.6). The runner reads
/// only pending mail, so a turn with nothing to do reads no message.
fn understand_loop(store: Arc<Store>) {
    loop {
        match crate::cost::after_sync(&store, now_ms()) {
            Ok(crate::cost::AfterSync::Busy) => {
                std::thread::sleep(BUSY_RETRY);
                continue;
            }
            Ok(_) => {}
            Err(e) => say!("iris: understand: {e}"),
        }
        std::thread::sleep(UNDERSTAND_EVERY);
    }
}

/// The time between two turns of the attention thread.
pub const ATTENTION_EVERY: Duration = Duration::from_secs(10);

/// The attention thread (spec 2 section 6.4): each [`ATTENTION_EVERY`], the
/// attention of the items whose origin message changed, and each hour of
/// each item ([`iris_policy::refresh_attention`]). It is apart from the
/// understanding thread, because one understanding run can take hours.
fn attention_loop(store: Arc<Store>) {
    loop {
        if let Err(e) = iris_policy::refresh_attention(&store, now_ms().div_euclid(1000)) {
            say!("iris: attention: {e}");
        }
        // The measure "time to the first brief" ends at the first brief with content.
        if let Err(e) = iris_api::measures::check_brief(&store, now_ms()) {
            say!("iris: measures: {e}");
        }
        std::thread::sleep(ATTENTION_EVERY);
    }
}

/// The schedule: one sync thread for each account, so a long first sync of
/// one account does not stop the syncs of the other accounts.
pub struct Engine {
    store: Arc<Store>,
    /// Each worker gets its own client: the Gmail quota is per mailbox.
    gmail: iris_gmail::Config,
    test_token_uri: Option<String>,
    workers: HashMap<i64, JoinHandle<()>>,
    last_clean: Option<Instant>,
    understander: Option<JoinHandle<()>>,
    attention: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn new(
        store: Arc<Store>,
        gmail: iris_gmail::Config,
        test_token_uri: Option<String>,
    ) -> Engine {
        Engine {
            store,
            gmail,
            test_token_uri,
            workers: HashMap::new(),
            last_clean: None,
            understander: None,
            attention: None,
        }
    }

    /// Start a sync thread for each account that has none, open the
    /// deferred commitments whose date came, expire the changeset actions
    /// 14 days after their submit, and run the content cleanup if
    /// [`CLEAN_EVERY`] passed. So an account that
    /// `iris account add` adds or signs in again starts to sync without a
    /// restart. Gives the accounts that got a new thread.
    pub fn tick(&mut self) -> Result<Vec<i64>> {
        self.workers.retain(|_, w| !w.is_finished());
        let mut started = Vec::new();
        for account in sync_accounts(&self.store)? {
            if self.workers.contains_key(&account) {
                continue;
            }
            let worker = Worker::new(
                self.store.clone(),
                Gmail::new(self.gmail.clone()),
                account,
                self.test_token_uri.clone(),
            );
            let handle = std::thread::Builder::new()
                .name(format!("sync-{account}"))
                .spawn(move || worker.run())
                .map_err(iris_store::Error::Io)?;
            self.workers.insert(account, handle);
            started.push(account);
        }
        if self.understander.as_ref().is_none_or(|h| h.is_finished()) {
            let store = self.store.clone();
            let handle = std::thread::Builder::new()
                .name("understand".into())
                .spawn(move || understand_loop(store))
                .map_err(iris_store::Error::Io)?;
            self.understander = Some(handle);
        }
        if self.attention.as_ref().is_none_or(|h| h.is_finished()) {
            let store = self.store.clone();
            let handle = std::thread::Builder::new()
                .name("attention".into())
                .spawn(move || attention_loop(store))
                .map_err(iris_store::Error::Io)?;
            self.attention = Some(handle);
        }
        if let Err(e) = self.store.reopen_deferred(now_ms()) {
            say!("iris: deferred commitments: {e}");
        }
        if let Err(e) = iris_changeset::expire(&self.store, now_ms()) {
            say!("iris: changeset expiry: {e}");
        }
        // The state verbs change no mailbox, so they apply here, with no
        // account and no token (R2-10).
        if let Err(e) = iris_apply::apply_states(&self.store) {
            say!("iris: state actions: {e}");
        }
        // Triage (R2-05): store the proposal of each account. A run with no
        // change writes nothing.
        for account in sync_accounts(&self.store)? {
            if let Err(e) = iris_triage::run(&self.store, account, now_ms()) {
                say!("iris: account {account}: triage: {e}");
            }
        }
        if self.last_clean.is_none_or(|t| t.elapsed() >= CLEAN_EVERY) {
            self.last_clean = Some(Instant::now());
            if let Err(e) = self.store.clean_gone(now_ms()) {
                say!("iris: content cleanup: {e}");
            }
        }
        Ok(started)
    }

    /// Run [`Engine::tick`] each [`SYNC_EVERY`] until the process ends.
    pub fn run(mut self) {
        loop {
            if let Err(e) = self.tick() {
                say!("iris: schedule: {e}");
            }
            std::thread::sleep(SYNC_EVERY);
        }
    }
}

/// The data folder: the `--data` setting, else the default of
/// specification 1, section 5.1.
pub fn data_dir(setting: Option<PathBuf>) -> std::io::Result<PathBuf> {
    setting
        .or_else(iris_store::default_data_dir)
        .ok_or_else(|| std::io::Error::other("no data folder: set --data or IRIS_DATA"))
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
