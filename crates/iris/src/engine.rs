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

    /// Sync the account one time. On HTTP 401, get a new access token one
    /// time and sync again. If the new token gets HTTP 401 too, the account
    /// needs a new sign-in (specification 1, section 4.7).
    pub fn sync(&mut self) -> Result<iris_sync::Synced> {
        for _ in 0..2 {
            let token = self.access()?;
            match iris_sync::sync(&self.store, &self.gmail, self.account, token.expose()) {
                Err(iris_sync::Error::Gmail(iris_gmail::Error::Unauthorized)) => {
                    self.token = None;
                }
                r => return Ok(r?),
            }
        }
        mark_needs_sign_in(&self.store, self.account, &self.sealed)?;
        Err(Error::NeedsSignIn)
    }

    /// A valid access token: the token in memory, or a new one from the
    /// stored sign-in.
    fn access(&mut self) -> Result<Secret> {
        if let Some(t) = &self.token {
            if t.expires_at > Instant::now() + TOKEN_MARGIN {
                return Ok(t.token.clone());
            }
        }
        let (mut client, refresh, sealed) = load_grant(&self.store, self.account)?;
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
                Err(e) => eprintln!("iris: account {}: {e}", self.account),
            }
            match sync_accounts(&self.store) {
                Ok(a) if !a.contains(&self.account) => return,
                _ => {}
            }
            std::thread::sleep(SYNC_EVERY);
        }
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
        }
    }

    /// Start a sync thread for each account that has none, and run the
    /// content cleanup if [`CLEAN_EVERY`] passed. So an account that
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
        if self.last_clean.is_none_or(|t| t.elapsed() >= CLEAN_EVERY) {
            self.last_clean = Some(Instant::now());
            if let Err(e) = self.store.clean_gone(now_ms()) {
                eprintln!("iris: content cleanup: {e}");
            }
        }
        Ok(started)
    }

    /// Run [`Engine::tick`] each [`SYNC_EVERY`] until the process ends.
    pub fn run(mut self) {
        loop {
            if let Err(e) = self.tick() {
                eprintln!("iris: schedule: {e}");
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
