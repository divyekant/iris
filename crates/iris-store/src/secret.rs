//! Secrets (specification 1, section 5.4): XChaCha20-Poly1305 with a 32-byte
//! key in the file `key`, mode 0600. A sealed value is the 24-byte nonce and
//! then the cipher text with its tag.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::{Error, Result};

const NONCE: usize = 24;

fn random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| Error::Io(io::Error::other(e.to_string())))
}

/// Read the key, or make it at the first start.
///
/// The first start writes a new key to its own temporary file, syncs it, and
/// then links it to `key`. So `key` is complete or not there, also after a
/// crash. The link fails if `key` exists, so a start never replaces a key.
/// Each start then reads `key` from disk, so all starts use the same key.
pub(crate) fn load_or_make_key(path: &Path) -> Result<[u8; 32]> {
    let read = || -> Result<[u8; 32]> {
        fs::read(path)?
            .try_into()
            .map_err(|_| Error::Key("the key file is not 32 bytes"))
    };
    if path.exists() {
        return read();
    }
    let dir = path.parent().expect("the key is in the data folder");
    let mut key = [0u8; 32];
    random(&mut key)?;
    let mut name = [0u8; 8];
    random(&mut name)?;
    let tmp = dir.join(format!("key.tmp-{}", crate::content::hex(&name)));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    let written = f.write_all(&key).and_then(|()| f.sync_all());
    drop(f);
    let linked = written.and_then(|()| fs::hard_link(&tmp, path));
    // This start made the file, so only this start removes it.
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => crate::sync_dir(dir)?,
        // Another start made the key first.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    read()
}

pub(crate) fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE];
    random(&mut nonce)?;
    let sealed = XChaCha20Poly1305::new(key.into())
        .encrypt(&XNonce::from(nonce), plain)
        .map_err(|_| Error::Key("encryption failed"))?;
    Ok([&nonce[..], &sealed].concat())
}

pub(crate) fn unseal(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE {
        return Err(Error::Key("the sealed value is too short"));
    }
    let (nonce, data) = sealed.split_at(NONCE);
    let nonce: [u8; NONCE] = nonce.try_into().expect("24 bytes");
    // The error does not say more: the secret and the key stay out of logs.
    XChaCha20Poly1305::new(key.into())
        .decrypt(&XNonce::from(nonce), data)
        .map_err(|_| Error::Key("the secret does not decrypt with this key"))
}
