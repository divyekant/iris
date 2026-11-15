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
pub(crate) fn load_or_make_key(path: &Path) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    match opts.open(path) {
        Ok(mut f) => {
            random(&mut key)?;
            f.write_all(&key)?;
            f.sync_all()?;
            Ok(key)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let bytes = fs::read(path)?;
            key = bytes
                .try_into()
                .map_err(|_| Error::Key("the key file is not 32 bytes"))?;
            Ok(key)
        }
        Err(e) => Err(e.into()),
    }
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
