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
use iris_provider::Provider;
use iris_store::Store;
use serde::{Deserialize, Serialize};

/// `account.state` of an account that syncs.
pub const STATE_OK: &str = "ok";
/// `account.state` when Google refused the stored sign-in. The person must
/// run `iris account add` again (specification 1, section 4.7).
pub const STATE_NEEDS_SIGN_IN: &str = "needs_sign_in";
/// `account.state` of an account that syncs, but whose sign-in does not
/// allow Gmail writes. Its approved actions wait for
/// `iris account add <client.json> --write`
/// (docs/specs/2026-11-18-gmail-write-access.md).
pub const STATE_READ_ONLY: &str = "read_only";

/// The time between two incremental syncs (specification 1, section 4.6).
pub const SYNC_EVERY: Duration = Duration::from_secs(60);

/// How often a sleeping sync thread looks for a new send mark of the write
/// view (docs/specs/2026-11-17-write.md, section 7).
pub const WAKE_EVERY: Duration = Duration::from_secs(1);
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
    /// The apply step (docs/specs/2026-11-17-held-outbox.md, section 4).
    Apply(iris_apply::Error),
    /// The account has no stored sign-in, or the provider refused it.
    NeedsSignIn,
    /// The account row cannot give a provider: an unknown provider, an IMAP
    /// account with no server, or a test setting that is not valid.
    Setup(String),
    /// The sign-in of the account does not allow Gmail writes.
    ReadOnly,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Auth(e) => write!(f, "sign-in: {e}"),
            Error::Gmail(e) => write!(f, "Gmail: {e}"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Sync(e) => write!(f, "sync: {e}"),
            Error::Apply(e) => write!(f, "apply: {e}"),
            Error::NeedsSignIn => f.write_str(
                "the account needs a new sign-in: run `iris account add <client.json>` (Gmail) \
                 or `iris account add-imap` (IMAP) or `iris account add-outlook` (Outlook)",
            ),
            Error::Setup(why) => write!(f, "account: {why}"),
            Error::ReadOnly => f.write_str(
                "the account has read access only, so its approved changes wait: run \
                 `iris account add <client.json> --write` to let Iris change mail",
            ),
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
    /// True if Google granted `gmail.modify`. A grant of an older Iris has
    /// no field: it had `gmail.readonly` only.
    #[serde(default)]
    write: bool,
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
        write: tokens.write,
    };
    let plain = serde_json::to_vec(&grant).expect("a grant serializes");
    let sealed = store.seal(&plain)?;
    let profile = gmail.profile(tokens.access.token.expose())?;
    let address = profile.email_address;
    // Refused if the address has an account of another provider.
    let state = if tokens.write {
        STATE_OK
    } else {
        STATE_READ_ONLY
    };
    let id = store.save_account_secret("gmail", &address, None, &sealed, state)?;
    Ok((id, address))
}

/// The server and the user name of an IMAP account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImapServer {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// The time-out of each connect and each read of an IMAP connection.
const IMAP_TIMEOUT: Duration = Duration::from_secs(60);

/// Save an IMAP account: the password sealed in `account.token`, in one
/// write with the server. Refused if the address has an account of another
/// provider or of another server or user (IMAP spec section 8). The caller
/// checks the password with one sign-in first. Gives the account ID.
pub fn save_imap(
    store: &Store,
    address: &str,
    server: &ImapServer,
    password: &Secret,
) -> Result<i64> {
    let sealed = store.seal(password.expose().as_bytes())?;
    let imap = (server.host.as_str(), server.port, server.user.as_str());
    Ok(store.save_account_secret("imap", address, Some(imap), &sealed, STATE_OK)?)
}

/// The saved server of the IMAP account with `address`, or `None` if no
/// IMAP account has the address.
pub fn saved_imap(store: &Store, address: &str) -> Result<Option<ImapServer>> {
    use rusqlite::OptionalExtension;
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT imap_host, imap_port, imap_user FROM account
             WHERE address = ?1 AND provider = 'imap' AND imap_host IS NOT NULL",
            [address],
            |r| {
                Ok(ImapServer {
                    host: r.get(0)?,
                    port: r.get(1)?,
                    user: r.get(2)?,
                })
            },
        )
        .optional()?)
    })?)
}

/// The sealed sign-in of an Outlook account: the app registration of the
/// user and the newest refresh token (Outlook spec section 7).
#[derive(Serialize, Deserialize)]
struct OutlookGrant {
    client_id: String,
    tenant: String,
    refresh: String,
}

/// Save a completed Microsoft sign-in of `address`: the client ID, the
/// tenant and the refresh token sealed in `account.token`, in one write.
/// Refused if the address has an account of another provider. Gives the
/// account ID.
pub fn save_outlook(
    store: &Store,
    address: &str,
    client_id: &str,
    tenant: &str,
    refresh: &Secret,
) -> Result<i64> {
    let grant = OutlookGrant {
        client_id: client_id.to_string(),
        tenant: tenant.to_string(),
        refresh: refresh.expose().to_string(),
    };
    let sealed = store.seal(&serde_json::to_vec(&grant).expect("a grant serializes"))?;
    Ok(store.save_account_secret("outlook", address, None, &sealed, STATE_OK)?)
}

/// The Graph setting: the real API, or the fake server of a test
/// (`IRIS_TEST_GRAPH_URL`, only `http://127.0.0.1:<port>`).
pub fn graph_config() -> Result<iris_graph::Config> {
    let mut config = iris_graph::Config::default();
    if let Some(url) = std::env::var_os("IRIS_TEST_GRAPH_URL") {
        let url = url.into_string().unwrap_or_default();
        if !crate::is_local_url(&url) {
            return Err(Error::Setup(
                "IRIS_TEST_GRAPH_URL must be http://127.0.0.1:<port>".into(),
            ));
        }
        config.base_url = url;
    }
    Ok(config)
}

/// The Outlook provider: one value for each account, which keeps the
/// folder IDs and the category switch of its mailbox. `sent` gives the
/// sent copies of the messages that Iris sent: only these get `SENT`
/// (Outlook spec section 6).
pub fn outlook_provider(sent: Vec<String>) -> Result<Box<dyn Provider>> {
    Ok(Box::new(
        iris_graph::Outlook::new(iris_graph::Graph::new(graph_config()?)).with_sent(sent),
    ))
}

/// The Outlook provider of `account`: the sent copies of Iris and the
/// stored category switch (limit #675). When Graph keeps other categories
/// than a write asks for, the provider stores the switch with the account,
/// so it lasts after a restart.
fn outlook_of(store: &Arc<Store>, account: i64) -> Result<Box<dyn Provider>> {
    let off: Option<i64> = store.read(|c| {
        Ok(c.query_row(
            "SELECT categories_off_at FROM account WHERE id = ?1",
            [account],
            |r| r.get(0),
        )?)
    })?;
    let keep = store.clone();
    let stored = move || {
        // A failed write keeps the switch in memory, and the provider tries
        // the write again at each refused category write (issue #717).
        keep.write(|tx| {
            Ok(tx.execute(
                "UPDATE account SET categories_off_at = ?2
                 WHERE id = ?1 AND categories_off_at IS NULL",
                (account, now_ms()),
            )?)
        })
        .inspect_err(|e| say!("iris: the Outlook category switch: {e}"))
        .is_ok()
    };
    // The store gives the sent copies at each read of labels: another
    // engine process can complete a send (issue #719).
    let read = store.clone();
    let sent = move || iris_apply::sent_by_iris(&read, account).map_err(|e| e.to_string());
    Ok(Box::new(
        iris_graph::Outlook::new(iris_graph::Graph::new(graph_config()?))
            .with_sent_source(sent)
            .with_category_switch(off.is_some(), stored),
    ))
}

/// The IMAP provider of the mailbox `address` on `server`: TLS to the
/// server. For tests, `IRIS_TEST_IMAP_PLAIN` set and the host `127.0.0.1`
/// give a plain TCP connection to a fake server. With the setting, any
/// other host is refused, so the password cannot go out with no TLS.
pub fn imap_provider(address: &str, server: &ImapServer) -> Result<Box<dyn Provider>> {
    let (host, port) = (server.host.clone(), server.port);
    if std::env::var_os("IRIS_TEST_IMAP_PLAIN").is_some() {
        if host != "127.0.0.1" {
            return Err(Error::Setup(
                "IRIS_TEST_IMAP_PLAIN needs the host 127.0.0.1".into(),
            ));
        }
        return Ok(Box::new(iris_imap::Imap::new(
            address,
            &server.user,
            move || {
                let s = std::net::TcpStream::connect((host.as_str(), port))?;
                s.set_nodelay(true)?;
                s.set_read_timeout(Some(IMAP_TIMEOUT))?;
                Ok(s)
            },
        )));
    }
    Ok(Box::new(iris_imap::Imap::new(
        address,
        &server.user,
        move || iris_imap::connect(&host, port, IMAP_TIMEOUT),
    )))
}

/// The provider of `account`, from its row: one value for each account,
/// which the worker keeps. `gmail` is the Gmail setting.
pub fn provider_for(
    store: &Arc<Store>,
    account: i64,
    gmail: &iris_gmail::Config,
) -> Result<Box<dyn Provider>> {
    type Row = (String, String, Option<String>, Option<u16>, Option<String>);
    let (provider, address, host, port, user): Row = store.read(|c| {
        Ok(c.query_row(
            "SELECT provider, address, imap_host, imap_port, imap_user FROM account
             WHERE id = ?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?)
    })?;
    match (provider.as_str(), host, port, user) {
        ("gmail", ..) => Ok(Box::new(Gmail::new(gmail.clone()))),
        ("imap", Some(host), Some(port), Some(user)) => {
            imap_provider(&address, &ImapServer { host, port, user })
        }
        ("imap", ..) => Err(Error::Setup("the IMAP account has no server".into())),
        ("outlook", ..) => outlook_of(store, account),
        (p, ..) => Err(Error::Setup(format!("no provider {p:?} in this version"))),
    }
}

/// The sign-in in the sealed value of an account of `provider`: the
/// client, the refresh token, and for Outlook the grant to seal again with
/// a new refresh token.
fn grant_of(
    store: &Store,
    provider: &str,
    sealed: &[u8],
) -> Result<(Client, Secret, Option<OutlookGrant>)> {
    let plain = store.unseal(sealed)?;
    let bad = || iris_store::Error::Key("the stored sign-in is not valid");
    if provider == "outlook" {
        let grant: OutlookGrant = serde_json::from_slice(&plain).map_err(|_| bad())?;
        let client = Client::microsoft(&grant.client_id, &grant.tenant)?;
        return Ok((client, Secret::new(grant.refresh.clone()), Some(grant)));
    }
    let grant: Grant = serde_json::from_slice(&plain).map_err(|_| bad())?;
    let client = Client::from_json(grant.client.get().as_bytes())?;
    Ok((client, Secret::new(grant.refresh), None))
}

/// Put `next` in `account.token`, but only while it is still `sealed`: a
/// new sign-in in the meantime keeps its token. Gives true if it changed.
fn replace_token(store: &Store, account: i64, sealed: &[u8], next: &[u8]) -> Result<bool> {
    Ok(store.write(|tx| {
        Ok(tx.execute(
            "UPDATE account SET token = ?3 WHERE id = ?1 AND token = ?2",
            (account, sealed, next),
        )? > 0)
    })?)
}

/// Read the sealed sign-in of `account`: the client, the refresh token, and
/// the sealed value itself.
fn load_grant(store: &Store, account: i64) -> Result<(Client, Secret, Vec<u8>, bool)> {
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
    Ok((client, Secret::new(grant.refresh), sealed, grant.write))
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

/// Set `account` to "read only", but only while `account.token` is still
/// `sealed` and the state is not "needs sign-in". Gives true if the state
/// changed.
pub fn mark_read_only(store: &Store, account: i64, sealed: &[u8]) -> Result<bool> {
    Ok(store.write(|tx| {
        Ok(tx.execute(
            "UPDATE account SET state = ?2 WHERE id = ?1 AND token = ?3
             AND (state IS NULL OR state NOT IN (?2, ?4))",
            (account, STATE_READ_ONLY, sealed, STATE_NEEDS_SIGN_IN),
        )? > 0)
    })?)
}

/// True if the apply of `account` may write to Gmail: its stored grant has
/// `gmail.modify` and no Gmail scope refusal set it to "read only" since.
/// Else it sets the state to "read only" (guarded as [`mark_read_only`])
/// and gives `Err(changed)`.
fn may_write(store: &Store, account: i64) -> Result<std::result::Result<(), bool>> {
    // The read-only state is a Gmail scope. An IMAP password allows the
    // flag writes.
    let provider: String = store.read(|c| {
        Ok(c.query_row(
            "SELECT provider FROM account WHERE id = ?1",
            [account],
            |r| r.get(0),
        )?)
    })?;
    if provider != "gmail" {
        return Ok(Ok(()));
    }
    let (_, _, sealed, write) = load_grant(store, account)?;
    let state: Option<String> = store.read(|c| {
        Ok(
            c.query_row("SELECT state FROM account WHERE id = ?1", [account], |r| {
                r.get(0)
            })?,
        )
    })?;
    if write && state.as_deref() != Some(STATE_READ_ONLY) {
        return Ok(Ok(()));
    }
    Ok(Err(mark_read_only(store, account, &sealed)?))
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
    mail: Box<dyn Provider>,
    account: i64,
    test_token_uri: Option<String>,
    token: Option<AccessToken>,
    /// When `token` came.
    got: Instant,
    /// The sealed sign-in that gave `token`.
    sealed: Vec<u8>,
}

impl Worker {
    /// `mail` is the provider of the account. `test_token_uri` is for
    /// tests: a token endpoint on 127.0.0.1.
    pub fn new(
        store: Arc<Store>,
        mail: impl Provider + 'static,
        account: i64,
        test_token_uri: Option<String>,
    ) -> Worker {
        Worker::boxed(store, Box::new(mail), account, test_token_uri)
    }

    /// [`Worker::new`] with the provider of [`provider_for`].
    pub fn boxed(
        store: Arc<Store>,
        mail: Box<dyn Provider>,
        account: i64,
        test_token_uri: Option<String>,
    ) -> Worker {
        Worker {
            store,
            mail,
            account,
            test_token_uri,
            token: None,
            got: Instant::now(),
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

    /// Call `f` with an access token. On HTTP 401, get a new access token
    /// and call again. A 401 from a token that ran out during the call (a
    /// call of a long first sync can run for hours) is not a refusal: the
    /// call continues with a new token. If two tokens are refused, the
    /// account needs a new sign-in (specification 1, section 4.7).
    fn call<T>(
        &mut self,
        f: impl Fn(&Store, &dyn Provider, i64, &str) -> iris_sync::Result<T>,
    ) -> Result<T> {
        let mut refused = 0;
        while refused < 2 {
            let token = self.access()?;
            match f(
                &self.store,
                self.mail.as_ref(),
                self.account,
                token.expose(),
            ) {
                Err(iris_sync::Error::Provider(iris_provider::Error::Unauthorized)) => {
                    let expires_at = self.token.take().map(|t| t.expires_at);
                    if !expires_at.is_some_and(|e| ran_out(self.got, e, Instant::now())) {
                        refused += 1;
                    }
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
        access(
            &self.store,
            self.account,
            self.test_token_uri.as_deref(),
            &mut self.token,
            &mut self.got,
            &mut self.sealed,
        )
    }

    /// The apply step after a sync (docs/specs/2026-11-17-held-outbox.md,
    /// section 4): the recovery, the approved Gmail actions, the held
    /// outbox and the release of each send whose hold ended. Gives the end
    /// of the next hold, and a report line for each action that did not
    /// end well. An account that needs a new sign-in gets no step.
    pub fn apply_step(&mut self) -> Result<Turn> {
        // No Gmail write for an account that needs a new sign-in, also
        // when a refreshed token works again: the person must sign in.
        if !sync_accounts(&self.store)?.contains(&self.account) {
            return Err(Error::NeedsSignIn);
        }
        // A read-only account gets only the recovery, which reads: its
        // actions wait. The error goes out once, when the state changes.
        let write = may_write(&self.store, self.account)?;
        let first = self.access()?;
        let (store, account, uri) = (&self.store, self.account, self.test_token_uri.as_deref());
        let (token, got, sealed) = (&mut self.token, &mut self.got, &mut self.sealed);
        // The error of the last renewal. A refusal by Google marks the
        // account in `access`; any other error keeps the account.
        let mut renew_error: Option<Error> = None;
        let (step, refused) = {
            // After HTTP 401 the apply asks for a new token: the token in
            // memory goes, and the stored sign-in gives a new one.
            let mut renew = || {
                *token = None;
                match access(store, account, uri, token, got, sealed) {
                    Ok(t) => {
                        renew_error = None;
                        Some(t.expose().to_string())
                    }
                    Err(e) => {
                        renew_error = Some(e);
                        None
                    }
                }
            };
            let mut t = iris_apply::Token {
                token: first.expose().to_string(),
                renew: &mut renew,
                refused: false,
            };
            let mail = self.mail.as_ref();
            let step = match write {
                Ok(()) => iris_apply::step(store, mail, &mut t, account, now_ms()),
                Err(_) => iris_apply::recover_step(store, mail, &mut t, account),
            };
            (step, t.refused)
        };
        // The token state comes before the result of the step: a refusal
        // during the recovery also ends with an error of the step.
        if refused {
            if let Some(e) = renew_error {
                // No new token: `NeedsSignIn` if Google refused the stored
                // sign-in, else an error that the next turn tries again.
                return Err(e);
            }
            // Gmail refused two tokens (specification 1, section 4.7).
            mark_needs_sign_in(&self.store, account, &self.sealed)?;
            return Err(Error::NeedsSignIn);
        }
        let step = step.map_err(Error::Apply)?;
        if write == Err(true) {
            return Err(Error::ReadOnly);
        }
        // Gmail refused a write for a missing scope: the later actions wait.
        if step.scope_refused {
            mark_read_only(&self.store, account, &self.sealed)?;
        }
        Ok(Turn {
            next: step.next,
            reports: reports(&step.results),
        })
    }

    /// Sync and run the apply step each [`SYNC_EVERY`], or at the end of
    /// the next hold if it comes first, until the account leaves the list
    /// of [`sync_accounts`]. Each error goes to standard error.
    pub fn run(mut self) {
        // False after an early wake for a written mail: the turn runs only
        // the apply step, and the sync keeps its time.
        let mut sync = true;
        let mut until = Instant::now();
        loop {
            if sync {
                self.sync_turn();
            }
            // Read before the step, so a mark that commits during the step wakes the next one.
            // With no read, no early wake: the turn keeps its normal time.
            let seen = iris_changeset::last_mark(&self.store).unwrap_or(i64::MAX);
            let next = self.apply_turn();
            match sync_accounts(&self.store) {
                Ok(a) if !a.contains(&self.account) => return,
                _ => {}
            }
            let end = Instant::now() + wait(next, now_ms());
            until = if sync { end } else { until.min(end) };
            sync = !self.pause(until, seen);
        }
    }

    /// Sleep until `until`, and look each [`WAKE_EVERY`] for a send mark of
    /// the write view newer than `seen` (docs/specs/2026-11-17-write.md,
    /// section 7). Gives true if a new mark woke the thread early.
    fn pause(&self, until: Instant, seen: i64) -> bool {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(left.min(WAKE_EVERY));
            if matches!(iris_changeset::written_since(&self.store, seen), Ok(true)) {
                return true;
            }
        }
    }

    fn sync_turn(&mut self) {
        match self.sync() {
            Ok(_) => {}
            // Another process syncs the account: not an error.
            Err(Error::Sync(iris_sync::Error::OtherSync)) => {}
            Err(e) => say!("iris: account {}: {e}", self.account),
        }
    }

    /// The apply step of a turn. Gives the end of the next hold.
    fn apply_turn(&mut self) -> Option<i64> {
        match self.apply_step() {
            Ok(turn) => {
                for line in &turn.reports {
                    say!("iris: account {}: {line}", self.account);
                }
                turn.next
            }
            // Another apply runs now: the next turn tries again.
            Err(Error::Apply(iris_apply::Error::Busy)) => None,
            Err(e) => {
                say!("iris: account {}: {e}", self.account);
                None
            }
        }
    }
}

/// The result of one apply step of a sync thread.
#[derive(Debug, PartialEq, Eq)]
pub struct Turn {
    /// The end of the next hold, in milliseconds since 1970.
    pub next: Option<i64>,
    /// One line for each action that failed or gave an error, for the
    /// standard error of the engine.
    pub reports: Vec<String>,
}

/// The report lines of the results of an apply step: each error, each
/// action that Gmail refused (`failed`), and each write with an unknown
/// result that the recovery decides later.
pub fn reports(results: &[(i64, iris_apply::Result<iris_apply::Outcome>)]) -> Vec<String> {
    use iris_apply::Outcome;
    results
        .iter()
        .filter_map(|(action, r)| match r {
            Err(e) => Some(format!("action {action}: {e}")),
            Ok(Outcome::Failed { error, .. }) => Some(format!("action {action} failed: {error}")),
            Ok(Outcome::Pending) => Some(format!(
                "action {action}: the result of the Gmail call is not known; the recovery decides"
            )),
            Ok(_) => None,
        })
        .collect()
}

/// A valid access token of `account`: `token` while the stored sign-in is
/// the one that gave it (`sealed`), else a new one from the stored sign-in.
fn access(
    store: &Store,
    account: i64,
    test_token_uri: Option<&str>,
    token: &mut Option<AccessToken>,
    got: &mut Instant,
    sealed_now: &mut Vec<u8>,
) -> Result<Secret> {
    // An IMAP account: the token of each call is the sealed password. It
    // has no refresh, so a refusal is a refusal (`Worker::call`).
    let (provider, sealed): (String, Option<Vec<u8>>) = store.read(|c| {
        Ok(c.query_row(
            "SELECT provider, token FROM account WHERE id = ?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    if provider == "imap" {
        let sealed = sealed.ok_or(Error::NeedsSignIn)?;
        let password = String::from_utf8(store.unseal(&sealed)?)
            .map_err(|_| iris_store::Error::Key("the stored password is not valid"))?;
        *token = None;
        *sealed_now = sealed;
        return Ok(Secret::new(password));
    }
    let sealed = sealed.ok_or(Error::NeedsSignIn)?;
    // A new sign-in (`iris account add`) replaces the token in memory too.
    if let Some(t) = token {
        if sealed == *sealed_now && t.expires_at > Instant::now() + TOKEN_MARGIN {
            return Ok(t.token.clone());
        }
    }
    let (mut client, refresh, outlook) = grant_of(store, &provider, &sealed)?;
    if let Some(uri) = test_token_uri {
        client = client.with_test_token_uri(uri)?;
    }
    let refreshed = if outlook.is_some() {
        client.refresh_tokens(&refresh)
    } else {
        client.refresh(&refresh).map(|access| iris_auth::Refreshed {
            access,
            refresh: None,
        })
    };
    match refreshed {
        Ok(r) => {
            let mut kept = sealed;
            // Microsoft gives a new refresh token at each refresh (Outlook
            // spec section 7): seal it in place of the old one. The old one
            // stays valid, so a crash before this write loses nothing.
            if let (Some(grant), Some(new)) = (outlook, r.refresh) {
                if new.expose() != grant.refresh {
                    let grant = OutlookGrant {
                        refresh: new.expose().to_string(),
                        ..grant
                    };
                    let next =
                        store.seal(&serde_json::to_vec(&grant).expect("a grant serializes"))?;
                    if !replace_token(store, account, &kept, &next)? {
                        // A new sign-in replaced the token: use this access
                        // token once, and read the new sign-in next time.
                        *token = None;
                        *sealed_now = kept;
                        return Ok(r.access.token);
                    }
                    kept = next;
                }
            }
            let new = r.access.token.clone();
            *token = Some(r.access);
            *got = Instant::now();
            *sealed_now = kept;
            Ok(new)
        }
        // The provider refused the refresh token: only a new sign-in helps.
        // A network or server error keeps the account, and the next turn
        // tries again (specification 1, section 4.7).
        Err(iris_auth::Error::Revoked) => {
            mark_needs_sign_in(store, account, &sealed)?;
            Err(Error::NeedsSignIn)
        }
        Err(e) => Err(e.into()),
    }
}

/// The wait of a sync thread after its turn: [`SYNC_EVERY`], or less if
/// the hold of a send ends before (`next`, in milliseconds since 1970).
/// Never less than one second.
pub fn wait(next: Option<i64>, now_ms: i64) -> Duration {
    let hold = next.map_or(SYNC_EVERY, |n| {
        Duration::from_millis(n.saturating_sub(now_ms).max(0) as u64)
    });
    hold.min(SYNC_EVERY).max(Duration::from_secs(1))
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
        std::thread::sleep(understand_turn(&store, now_ms()));
    }
}

/// One turn of the understanding thread. The answer is the time until the
/// next turn. After each run, idle or not, the proposal step of matters
/// runs with its hourly gate, so a mailbox that is already understood
/// still gets proposals. Then the nudge step ([`crate::nudge::step`]).
pub fn understand_turn(store: &Store, now_ms: i64) -> Duration {
    match crate::cost::after_sync(store, now_ms) {
        Ok(crate::cost::AfterSync::Busy) => return BUSY_RETRY,
        Ok(_) => {}
        Err(e) => say!("iris: understand: {e}"),
    }
    if let Err(e) = crate::cost::propose_matters(store, iris_matters::Gate::Hourly, now_ms) {
        say!("iris: matters: {e}");
    }
    let connect = |row: &iris_understand::budget::ModelRow| crate::cost::connect(store, row);
    if let Err(e) = crate::nudge::step(store, &connect, now_ms) {
        say!("iris: nudge: {e}");
    }
    UNDERSTAND_EVERY
}

/// The time between two turns of the attention thread.
pub const ATTENTION_EVERY: Duration = Duration::from_secs(10);

/// The attention thread (spec 2 section 6.4): each [`ATTENTION_EVERY`], the
/// attention of the items whose origin message changed, and each hour of
/// each item ([`iris_policy::refresh_attention`]). It is apart from the
/// understanding thread, because one understanding run can take hours.
/// The same thread computes again the conversations of the threads that a
/// change of a sender kind marked (`iris_understand::store::update_due_talks`),
/// and writes the rules that Iris proposes from the approvals
/// of the person (spec 2 section 6.6, [`iris_policy::propose::refresh`]),
/// and the notices by rule ([`iris_policy::notice::refresh`]).
fn attention_loop(store: Arc<Store>) {
    let mut mark = None;
    let open = {
        let store = store.clone();
        move |id| {
            store
                .read(|c| iris_policy::notice::is_open(c, id))
                .unwrap_or(false)
        }
    };
    let system = crate::notify::spawn(open, crate::notify::show);
    loop {
        // The system notice runs on its own thread: this loop never waits
        // for it.
        for n in attention_turn(&store, &mut mark) {
            let _ = system.try_send(n);
        }
        std::thread::sleep(ATTENTION_EVERY);
    }
}

/// One turn of the attention loop. Gives the notices for the system. Each
/// step logs its own error, so one failed step does not stop the others.
pub fn attention_turn(
    store: &Store,
    mark: &mut Option<iris_policy::propose::Mark>,
) -> Vec<crate::notify::Shown> {
    // The conversations first: a kind change marks their threads, and
    // the attention reads the items.
    if let Err(e) = iris_understand::store::update_due_talks(store, now_ms()) {
        say!("iris: conversations: {e}");
    }
    if let Err(e) = iris_policy::refresh_attention(store, now_ms().div_euclid(1000)) {
        say!("iris: attention: {e}");
    }
    if let Err(e) = iris_policy::propose::refresh(store, now_ms().div_euclid(1000), mark) {
        say!("iris: proposed rules: {e}");
    }
    // Notices by rule (R5-08): the row commits first, so a crash before
    // the system notice still leaves the notice in the web client.
    // The Mac app shows some notices: the engine chooses before the queue.
    let mut system: Vec<_> = match iris_policy::notice::refresh(store, now_ms()) {
        Ok(new) => crate::notify::for_system(new, now_ms())
            .iter()
            .map(crate::notify::Shown::rule)
            .collect(),
        Err(e) => {
            say!("iris: notices: {e}");
            Vec::new()
        }
    };
    // The due date of a job (decision D24): the move to needs you, then one
    // system notice for each moved job. The jobs page shows the state.
    match iris_job::move_due(store, now_ms()) {
        Ok(turn) => {
            for (job, e) in &turn.failed {
                say!("iris: due job j-{job}: {e}");
            }
            system.extend(turn.moved.iter().map(crate::notify::Shown::job_due));
        }
        Err(e) => say!("iris: due jobs: {e}"),
    }
    // The measure "time to the first brief" ends at the first brief with content.
    if let Err(e) = iris_api::measures::check_brief(store, now_ms()) {
        say!("iris: measures: {e}");
    }
    system
}

/// Delete the mail of each removed account, with the workspaces of its
/// jobs: [`iris_api::delete_removed`].
pub fn delete_removed(store: &Store, only: Option<i64>) -> Result<Option<iris_store::Deleted>> {
    Ok(iris_api::delete_removed(store, only)?)
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
    /// 14 days after their submit, check the watches, and run the content cleanup
    /// and the removal of the grants of ended jobs if [`CLEAN_EVERY`] passed. So an account that
    /// `iris account add` adds or signs in again starts to sync without a
    /// restart. Gives the accounts that got a new thread.
    pub fn tick(&mut self) -> Result<Vec<i64>> {
        self.workers.retain(|_, w| !w.is_finished());
        let mut started = Vec::new();
        for account in sync_accounts(&self.store)? {
            if self.workers.contains_key(&account) {
                continue;
            }
            let mail = match provider_for(&self.store, account, &self.gmail) {
                Ok(m) => m,
                Err(e) => {
                    say!("iris: account {account}: {e}");
                    continue;
                }
            };
            let worker = Worker::boxed(
                self.store.clone(),
                mail,
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
        // The delete of a removed account continues after a crash or a
        // held lock.
        if let Err(e) = delete_removed(&self.store, None) {
            say!("iris: delete of a removed account: {e}");
        }
        if let Err(e) = self.store.reopen_deferred(now_ms()) {
            say!("iris: deferred commitments: {e}");
        }
        if let Err(e) = self.store.expire_signals(now_ms()) {
            say!("iris: signal expiry: {e}");
        }
        if let Err(e) = iris_changeset::expire(&self.store, now_ms()) {
            say!("iris: changeset expiry: {e}");
        }
        // The state verbs change no mailbox, so they apply here, with no
        // account and no token (R2-10).
        if let Err(e) = iris_apply::apply_states(&self.store) {
            say!("iris: state actions: {e}");
        }
        // Watches (R5-07): the one notice of each watch whose mail came.
        if let Err(e) = self.store.check_watches(now_ms()) {
            say!("iris: watches: {e}");
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
            // The grants of an ended job match nothing: remove them from
            // the policy (issue #385). A held lock or a file with a change
            // to confirm waits for the next pass.
            if let Err(e) = iris_policy::end_grants(&self.store, now_ms().div_euclid(1000)) {
                say!("iris: grants of ended jobs: {e}");
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

/// True if a token that came at `got` and lives to `expires_at` ran out by
/// `now`: it was usable when it came, and its life (less [`TOKEN_MARGIN`])
/// is over. A token with a shorter life never counts, so a 401 on it is a
/// refusal, and the retries of [`Worker::call`] stay bounded.
fn ran_out(got: Instant, expires_at: Instant, now: Instant) -> bool {
    expires_at > got + TOKEN_MARGIN && now + TOKEN_MARGIN >= expires_at
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn only_a_usable_token_at_its_end_ran_out() {
        let got = Instant::now();
        let hour = got + Duration::from_secs(3600);
        // A 401 one hour later: the token ran out.
        assert!(ran_out(got, hour, got + Duration::from_secs(3590)));
        assert!(ran_out(got, hour, got + Duration::from_secs(7200)));
        // A 401 early in the life of the token: a refusal.
        assert!(!ran_out(got, hour, got));
        assert!(!ran_out(got, hour, got + Duration::from_secs(3000)));
        // A token with a life inside the margin was never usable.
        let short = got + TOKEN_MARGIN;
        assert!(!ran_out(got, short, got + Duration::from_secs(3600)));
    }
}
