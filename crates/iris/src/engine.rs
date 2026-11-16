//! The engine: the accounts, the tokens and the sync schedule
//! (specification 1, sections 4.6, 4.7 and 4.9).

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iris_auth::{AccessToken, Client, Secret};
use iris_gmail::Gmail;
use iris_store::Store;

/// `account.state` of an account that syncs.
pub const STATE_OK: &str = "ok";
/// `account.state` when Google refused the stored sign-in. The person must
/// run `iris account add` again (specification 1, section 4.7).
pub const STATE_NEEDS_SIGN_IN: &str = "needs_sign_in";

/// The time between two incremental syncs (specification 1, section 4.6).
pub const SYNC_EVERY: Duration = Duration::from_secs(60);
/// The time between two runs of the content cleanup (section 4.5).
pub const CLEAN_EVERY: Duration = Duration::from_secs(3600);

/// The file in the data folder that holds the sealed OAuth client file.
const CLIENT_FILE: &str = "google-client";

/// Get a new access token when the old one has less time than this.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub enum Error {
    Auth(iris_auth::Error),
    Gmail(iris_gmail::Error),
    Store(iris_store::Error),
    Sync(iris_sync::Error),
    /// No OAuth client is saved in the data folder.
    NoClient,
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
            Error::NoClient => {
                f.write_str("no OAuth client is saved: run `iris account add <client.json>`")
            }
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
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Save the OAuth client file in the data folder, sealed with the key of the
/// store (specification 1, section 5.4). One client serves each account.
pub fn save_client(store: &Store, json: &[u8]) -> Result<()> {
    let sealed = store.seal(json)?;
    let path = store.dir().join(CLIENT_FILE);
    let tmp = store.dir().join(format!("{CLIENT_FILE}.tmp"));
    let mut file = private_file(&tmp)?;
    file.write_all(&sealed)?;
    file.sync_all()?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Read the OAuth client that [`save_client`] saved.
pub fn load_client(store: &Store) -> Result<Client> {
    let sealed = match std::fs::read(store.dir().join(CLIENT_FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::NoClient),
        Err(e) => return Err(e.into()),
    };
    Ok(Client::from_json(&store.unseal(&sealed)?)?)
}

/// Copy the sealed OAuth client into the backup folder `dest`, after
/// `Store::backup`. The store does not know this file. `iris account add`
/// replaces the file with a rename, so the copy is one complete version.
pub fn backup_client(store: &Store, dest: &Path) -> Result<()> {
    match std::fs::read(store.dir().join(CLIENT_FILE)) {
        Ok(bytes) => {
            let mut file = private_file(&dest.join(CLIENT_FILE))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)
}

/// Save a completed sign-in: find the address of the mailbox, add the
/// account (or find the account with this address), and store the refresh
/// token sealed. Gives the account ID and the address.
pub fn save_sign_in(
    store: &Store,
    gmail: &Gmail,
    tokens: &iris_auth::Tokens,
) -> Result<(i64, String)> {
    let profile = gmail.profile(tokens.access.token.expose())?;
    let address = profile.email_address;
    let id = store.add_account("gmail", &address)?;
    let sealed = store.seal(tokens.refresh.expose().as_bytes())?;
    store.write(|tx| {
        tx.execute(
            "UPDATE account SET token = ?2, state = ?3 WHERE id = ?1",
            (id, &sealed, STATE_OK),
        )?;
        Ok(())
    })?;
    Ok((id, address))
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

/// The engine state between two syncs. The access tokens stay in memory only.
pub struct Engine {
    pub store: std::sync::Arc<Store>,
    pub gmail: Gmail,
    client: Option<Client>,
    tokens: HashMap<i64, AccessToken>,
    last_clean: Option<Instant>,
}

impl Engine {
    /// `client` is `None` when no account is added yet. The engine then
    /// syncs nothing.
    pub fn new(store: std::sync::Arc<Store>, gmail: Gmail, client: Option<Client>) -> Engine {
        Engine {
            store,
            gmail,
            client,
            tokens: HashMap::new(),
            last_clean: None,
        }
    }

    /// One turn of the schedule: sync each account, then run the content
    /// cleanup if [`CLEAN_EVERY`] passed. An error of one account does not
    /// stop the other accounts. Gives the result of each account.
    pub fn tick(&mut self) -> Vec<(i64, Result<iris_sync::Synced>)> {
        let accounts = match sync_accounts(&self.store) {
            Ok(a) => a,
            Err(e) => return vec![(0, Err(e))],
        };
        let out = accounts
            .into_iter()
            .map(|id| (id, self.sync_account(id)))
            .collect();
        if self.last_clean.is_none_or(|t| t.elapsed() >= CLEAN_EVERY) {
            self.last_clean = Some(Instant::now());
            if let Err(e) = self.store.clean_gone(now_ms()) {
                eprintln!("iris: content cleanup: {e}");
            }
        }
        out
    }

    /// Sync one account. On HTTP 401, get a new access token one time and
    /// sync again (specification 1, section 4.7).
    pub fn sync_account(&mut self, account: i64) -> Result<iris_sync::Synced> {
        let token = self.access(account)?;
        match iris_sync::sync(&self.store, &self.gmail, account, token.expose()) {
            Err(iris_sync::Error::Gmail(iris_gmail::Error::Unauthorized)) => {
                self.tokens.remove(&account);
                let token = self.access(account)?;
                match iris_sync::sync(&self.store, &self.gmail, account, token.expose()) {
                    Err(iris_sync::Error::Gmail(iris_gmail::Error::Unauthorized)) => {
                        self.tokens.remove(&account);
                        self.needs_sign_in(account)?;
                        Err(Error::NeedsSignIn)
                    }
                    r => Ok(r?),
                }
            }
            r => Ok(r?),
        }
    }

    /// A valid access token of `account`: the token in memory, or a new one
    /// from the stored refresh token.
    fn access(&mut self, account: i64) -> Result<Secret> {
        if let Some(t) = self.tokens.get(&account) {
            if t.expires_at > Instant::now() + TOKEN_MARGIN {
                return Ok(t.token.clone());
            }
        }
        let client = self.client.as_ref().ok_or(Error::NoClient)?;
        let sealed: Option<Vec<u8>> = self.store.read(|c| {
            Ok(
                c.query_row("SELECT token FROM account WHERE id = ?1", [account], |r| {
                    r.get(0)
                })?,
            )
        })?;
        let sealed = sealed.ok_or(Error::NeedsSignIn)?;
        let plain = self.store.unseal(&sealed)?;
        let refresh =
            Secret::new(String::from_utf8(plain).map_err(|_| iris_store::Error::Key("bad token"))?);
        match client.refresh(&refresh) {
            Ok(t) => {
                let token = t.token.clone();
                self.tokens.insert(account, t);
                Ok(token)
            }
            Err(iris_auth::Error::Revoked) => {
                self.needs_sign_in(account)?;
                Err(Error::NeedsSignIn)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn needs_sign_in(&self, account: i64) -> Result<()> {
        self.store.write(|tx| {
            tx.execute(
                "UPDATE account SET state = ?2 WHERE id = ?1",
                (account, STATE_NEEDS_SIGN_IN),
            )?;
            Ok(())
        })?;
        Ok(())
    }
}

/// Run the schedule until the process ends: a sync at once, then each
/// [`SYNC_EVERY`]. Each error goes to standard error.
pub fn run(mut engine: Engine) {
    loop {
        for (account, result) in engine.tick() {
            match result {
                Ok(_) => {}
                // Another process syncs the account: not an error.
                Err(Error::Sync(iris_sync::Error::OtherSync)) => {}
                Err(e) => eprintln!("iris: account {account}: {e}"),
            }
        }
        std::thread::sleep(SYNC_EVERY);
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
