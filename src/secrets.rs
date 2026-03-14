use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use once_cell::sync::Lazy;
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::RwLock;

const SECRETS_KEY_ENV: &str = "IRIS_SECRETS_KEY";
const ENCRYPTED_PREFIX: &str = "enc:v1:";
const SECRET_CONFIG_KEYS: &[&str] = &["anthropic_api_key", "openai_api_key", "memories_api_key"];
const SECRET_ACCOUNT_COLUMNS: &[&str] = &["access_token", "refresh_token", "password_encrypted"];

static RUNTIME_SECRETS_KEY: Lazy<RwLock<Option<[u8; 32]>>> = Lazy::new(|| RwLock::new(None));
#[cfg(test)]
static TEST_SECRETS_LOCK: Lazy<std::sync::Mutex<()>> = Lazy::new(|| std::sync::Mutex::new(()));

#[derive(Debug, Default, Clone, Copy)]
pub struct SecretMigrationReport {
    pub migrated_values: usize,
    pub plaintext_values_remaining: usize,
    pub encrypted_values_present: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("IRIS_SECRETS_KEY must be a 32-byte base64 or 64-character hex key")]
    InvalidKeyFormat,
    #[error("encrypted secrets exist but IRIS_SECRETS_KEY is not configured")]
    MissingKey,
    #[error("failed to encrypt secret")]
    Encrypt,
    #[error("failed to decrypt secret")]
    Decrypt,
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

pub fn configure_from_env() -> Result<(), SecretError> {
    let parsed = std::env::var(SECRETS_KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_key_material(&value))
        .transpose()?;

    *RUNTIME_SECRETS_KEY
        .write()
        .expect("secrets key lock poisoned") = parsed;
    Ok(())
}

pub fn encrypt_secret_for_storage(value: &str) -> Result<String, SecretError> {
    let Some(key) = current_key() else {
        return Ok(value.to_string());
    };

    if is_encrypted_value(value) {
        return Ok(value.to_string());
    }

    Ok(encrypt_with_key(&key, value))
}

pub fn decrypt_secret_for_runtime(value: &str) -> Result<String, SecretError> {
    if !is_encrypted_value(value) {
        return Ok(value.to_string());
    }

    let key = current_key().ok_or(SecretError::MissingKey)?;
    decrypt_with_key(&key, value)
}

pub fn encrypt_optional_secret(value: Option<&str>) -> Result<Option<String>, SecretError> {
    value.map(encrypt_secret_for_storage).transpose()
}

pub fn decrypt_optional_secret(value: Option<String>) -> Result<Option<String>, SecretError> {
    value
        .map(|secret| decrypt_secret_for_runtime(&secret))
        .transpose()
}

pub fn get_secret_config_value(
    conn: &Connection,
    key: &str,
) -> Result<Option<String>, SecretError> {
    let stored = conn
        .query_row(
            "SELECT value FROM config WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    decrypt_optional_secret(stored)
}

pub fn set_secret_config_value(
    conn: &Connection,
    key: &str,
    value: &str,
) -> Result<(), SecretError> {
    let stored = encrypt_secret_for_storage(value)?;
    conn.execute(
        "INSERT INTO config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = unixepoch()",
        params![key, stored],
    )?;
    Ok(())
}

pub fn migrate_persisted_secrets(conn: &Connection) -> Result<SecretMigrationReport, SecretError> {
    let mut report = SecretMigrationReport::default();
    let has_key = current_key().is_some();

    for key in SECRET_CONFIG_KEYS {
        let stored = conn
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let Some(value) = stored else {
            continue;
        };

        if value.is_empty() {
            continue;
        }

        if is_encrypted_value(&value) {
            report.encrypted_values_present += 1;
            if has_key {
                decrypt_secret_for_runtime(&value)?;
            }
            continue;
        }

        if has_key {
            set_secret_config_value(conn, key, &value)?;
            report.migrated_values += 1;
        } else {
            report.plaintext_values_remaining += 1;
        }
    }

    for column in SECRET_ACCOUNT_COLUMNS {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, {column} FROM accounts WHERE {column} IS NOT NULL AND {column} != ''"
        ))?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<_, _>>()?;

        for (id, value) in rows {
            if is_encrypted_value(&value) {
                report.encrypted_values_present += 1;
                if has_key {
                    decrypt_secret_for_runtime(&value)?;
                }
                continue;
            }

            if has_key {
                let encrypted = encrypt_secret_for_storage(&value)?;
                conn.execute(
                    &format!(
                        "UPDATE accounts SET {column} = ?1, updated_at = unixepoch() WHERE id = ?2"
                    ),
                    params![encrypted, id],
                )?;
                report.migrated_values += 1;
            } else {
                report.plaintext_values_remaining += 1;
            }
        }
    }

    if report.encrypted_values_present > 0 && !has_key {
        return Err(SecretError::MissingKey);
    }

    Ok(report)
}

pub fn is_encrypted_value(value: &str) -> bool {
    value.starts_with(ENCRYPTED_PREFIX)
}

fn current_key() -> Option<[u8; 32]> {
    *RUNTIME_SECRETS_KEY
        .read()
        .expect("secrets key lock poisoned")
}

fn parse_key_material(value: &str) -> Result<[u8; 32], SecretError> {
    let trimmed = value.trim();
    let decoded = decode_base64_key(trimmed)
        .or_else(|| decode_hex(trimmed).ok())
        .ok_or(SecretError::InvalidKeyFormat)?;

    if decoded.len() != 32 {
        return Err(SecretError::InvalidKeyFormat);
    }

    let mut key = [0_u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn decode_base64_key(value: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(value)
        .ok()
        .filter(|decoded| decoded.len() == 32)
        .or_else(|| {
            STANDARD_NO_PAD
                .decode(value)
                .ok()
                .filter(|decoded| decoded.len() == 32)
        })
}

fn decode_hex(value: &str) -> Result<Vec<u8>, SecretError> {
    if value.len() != 64 {
        return Err(SecretError::InvalidKeyFormat);
    }

    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let hex = std::str::from_utf8(pair).map_err(|_| SecretError::InvalidKeyFormat)?;
            u8::from_str_radix(hex, 16).map_err(|_| SecretError::InvalidKeyFormat)
        })
        .collect()
}

fn encrypt_with_key(key_bytes: &[u8; 32], plaintext: &str) -> String {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key_bytes));
    let mut nonce_bytes = [0_u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("XChaCha20Poly1305 encryption should not fail");

    format!(
        "{ENCRYPTED_PREFIX}{}:{}",
        STANDARD.encode(nonce_bytes),
        STANDARD.encode(ciphertext)
    )
}

fn decrypt_with_key(key_bytes: &[u8; 32], encrypted: &str) -> Result<String, SecretError> {
    let payload = encrypted
        .strip_prefix(ENCRYPTED_PREFIX)
        .ok_or(SecretError::Decrypt)?;
    let (nonce_b64, ciphertext_b64) = payload.split_once(':').ok_or(SecretError::Decrypt)?;
    let nonce_bytes = STANDARD
        .decode(nonce_b64)
        .map_err(|_| SecretError::Decrypt)?;
    let ciphertext = STANDARD
        .decode(ciphertext_b64)
        .map_err(|_| SecretError::Decrypt)?;

    if nonce_bytes.len() != 24 {
        return Err(SecretError::Decrypt);
    }

    let cipher = XChaCha20Poly1305::new(Key::from_slice(key_bytes));
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce_bytes), ciphertext.as_ref())
        .map_err(|_| SecretError::Decrypt)?;
    String::from_utf8(plaintext).map_err(|_| SecretError::Decrypt)
}

#[cfg(test)]
pub fn set_runtime_key_for_tests(key: Option<[u8; 32]>) {
    *RUNTIME_SECRETS_KEY
        .write()
        .expect("secrets key lock poisoned") = key;
}

#[cfg(test)]
pub fn lock_test_runtime_key() -> std::sync::MutexGuard<'static, ()> {
    TEST_SECRETS_LOCK
        .lock()
        .expect("test secrets lock poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_secret_with_explicit_key() {
        let _lock = lock_test_runtime_key();
        let key = [7_u8; 32];
        let encrypted = encrypt_with_key(&key, "secret-value");
        assert!(is_encrypted_value(&encrypted));
        let decrypted = decrypt_with_key(&key, &encrypted).unwrap();
        assert_eq!(decrypted, "secret-value");
    }

    #[test]
    fn parse_hex_key_material() {
        let _lock = lock_test_runtime_key();
        let key =
            parse_key_material("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
                .unwrap();
        assert_eq!(key[0], 1);
        assert_eq!(key[31], 32);
    }

    #[test]
    fn parse_base64_key_material_without_padding() {
        let _lock = lock_test_runtime_key();
        let key = parse_key_material("AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA").unwrap();
        assert_eq!(key[0], 1);
        assert_eq!(key[31], 32);
    }
}
