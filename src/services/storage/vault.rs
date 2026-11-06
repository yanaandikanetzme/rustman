//! Authenticated encryption for local SQLite payloads. IDs/names remain metadata.
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

const PREFIX: &str = "rustman:aes256gcm:v1:";
// Serialize first-use creation, and avoid a native keychain call on every row.
static KEYS: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

fn key(conn: &Connection) -> Result<Vec<u8>, String> {
    let mut cache = KEYS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Database key cache unavailable")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_vault (name TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )
    .map_err(|e| e.to_string())?;
    let id: Option<String> = conn
        .query_row(
            "SELECT value FROM local_vault WHERE name='key_id'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (id, create) = match id {
        Some(id) => (id, false),
        None => (uuid::Uuid::new_v4().to_string(), true),
    };
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid database key identifier")?;
    if let Some(key) = cache.get(&id) {
        return Ok(key.clone());
    }
    let key = crate::services::keychain::key(&id, create)?;
    if create {
        conn.execute(
            "INSERT INTO local_vault (name,value) VALUES ('key_id',?1)",
            [&id],
        )
        .map_err(|e| e.to_string())?;
    }
    cache.insert(id, key.clone());
    Ok(key)
}

fn aad(table: &str, column: &str, id: &str) -> Vec<u8> {
    // JSON encodes boundaries unambiguously even for imported Unicode IDs.
    serde_json::to_vec(&("rustman-local-v1", table, column, id)).expect("string serialization")
}

fn encrypt(key: &[u8], aad: &[u8], value: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid database key")?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: value.as_bytes(),
                aad,
            },
        )
        .map_err(|_| "Database encryption failed")?;
    let mut bytes = nonce.to_vec();
    bytes.extend(ciphertext);
    Ok(format!("{PREFIX}{}", STANDARD_NO_PAD.encode(bytes)))
}

fn decrypt(key: &[u8], aad: &[u8], value: &str) -> Result<String, String> {
    let encoded = value
        .strip_prefix(PREFIX)
        .ok_or("Unencrypted or unsupported database payload")?;
    let bytes = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| "Invalid encrypted database payload")?;
    if bytes.len() < 28 {
        return Err("Invalid encrypted database payload".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid database key")?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&bytes[..12]),
            Payload {
                msg: &bytes[12..],
                aad,
            },
        )
        .map_err(|_| "Database authentication failed; data was not overwritten")?;
    String::from_utf8(plaintext).map_err(|_| "Invalid decrypted database payload".into())
}

pub fn seal(
    conn: &Connection,
    table: &str,
    column: &str,
    id: &str,
    value: &str,
) -> Result<String, String> {
    encrypt(&key(conn)?, &aad(table, column, id), value)
}
pub fn unseal(
    conn: &Connection,
    table: &str,
    column: &str,
    id: &str,
    value: &str,
) -> Result<String, String> {
    decrypt(&key(conn)?, &aad(table, column, id), value)
}

/// Upgrade legacy plaintext exactly once, atomically. Never interpret a plaintext
/// row inserted into an already-upgraded database as valid authenticated data.
pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA secure_delete=ON")
        .map_err(|e| e.to_string())?;
    let key = key(conn)?;
    let key_id: String = conn
        .query_row(
            "SELECT value FROM local_vault WHERE name='key_id'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let already_migrated = crate::services::keychain::migration_complete(&key_id)?;
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM local_vault WHERE name='format'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if version.as_deref().is_some_and(|v| v != "1") {
        return Err("Unsupported database encryption format".into());
    }
    if already_migrated && version.is_none() {
        return Err(
            "Database encryption format marker is missing; refusing plaintext downgrade".into(),
        );
    }
    if version.is_none() {
        let mut saw_plaintext = false;
        let mut saw_ciphertext = false;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for (table, pk, columns) in [
            ("environments", "id", &["variables"][..]),
            ("history", "id", &["url", "request_json"][..]),
            ("session", "key", &["data"][..]),
            ("requests", "id", &["data"][..]),
            ("request_drafts", "request_id", &["data", "base"][..]),
            ("request_examples", "id", &["data"][..]),
        ] {
            for column in columns {
                let rows: Vec<(String, String)> = {
                    let mut stmt = tx
                        .prepare(&format!("SELECT {pk},{column} FROM {table}"))
                        .map_err(|e| e.to_string())?;
                    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                        .map_err(|e| e.to_string())?
                        .collect::<Result<_, _>>()
                        .map_err(|e| e.to_string())?
                };
                for (id, value) in rows {
                    saw_ciphertext |= value.starts_with(PREFIX);
                    saw_plaintext |= !value.starts_with(PREFIX);
                    if saw_ciphertext && saw_plaintext {
                        return Err(
                            "Mixed plaintext/encrypted legacy state requires explicit recovery"
                                .into(),
                        );
                    }
                    let value = if value.starts_with(PREFIX) {
                        decrypt(&key, &aad(table, column, &id), &value)?;
                        value
                    } else {
                        encrypt(&key, &aad(table, column, &id), &value)?
                    };
                    tx.execute(
                        &format!("UPDATE {table} SET {column}=?1 WHERE {pk}=?2"),
                        params![value, id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        tx.execute_batch("INSERT INTO local_vault VALUES ('format','1'); INSERT OR REPLACE INTO local_vault VALUES ('cleanup_pending','1')")
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    // Authenticate every protected row before boot can expose a writable
    // connection. A failed load must never become an empty session auto-save.
    for (table, pk, columns) in [
        ("environments", "id", &["variables"][..]),
        ("history", "id", &["url", "request_json"][..]),
        ("session", "key", &["data"][..]),
        ("requests", "id", &["data"][..]),
        ("request_drafts", "request_id", &["data", "base"][..]),
        ("request_examples", "id", &["data"][..]),
    ] {
        for column in columns {
            let mut stmt = conn
                .prepare(&format!("SELECT {pk},{column} FROM {table}"))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (id, value) = row.map_err(|e| e.to_string())?;
                decrypt(&key, &aad(table, column, &id), &value)?;
            }
        }
    }
    let cleanup: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_vault WHERE name='cleanup_pending')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if cleanup {
        // Busy checkpoints are not success: old plaintext WAL may still exist.
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if busy != 0 {
            return Err(
                "Close other Rustman instances to finish encrypted storage migration".into(),
            );
        }
        conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE); DELETE FROM local_vault WHERE name='cleanup_pending';")
            .map_err(|e| e.to_string())?;
    }
    if !already_migrated {
        crate::services::keychain::mark_migrated(&key_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip_nonce_and_context() {
        let key = Aes256Gcm::generate_key(OsRng);
        let context = aad("environment", "variables", "一");
        let value = "synthetic-secret-ไทย-한국-一";
        let first = encrypt(&key, &context, value).unwrap();
        let second = encrypt(&key, &context, value).unwrap();
        assert_ne!(first, second);
        assert!(!first.contains(value));
        assert_eq!(decrypt(&key, &context, &first).unwrap(), value);
        assert!(decrypt(&key, &aad("history", "url", "一"), &first).is_err());
        assert!(decrypt(&key, &aad("environment", "variables", "two"), &first).is_err());
        assert!(decrypt(&[0; 32], &context, &first).is_err());
        let mut bytes = STANDARD_NO_PAD
            .decode(first.strip_prefix(PREFIX).unwrap())
            .unwrap();
        bytes[15] ^= 1;
        assert!(
            decrypt(
                &key,
                &context,
                &format!("{PREFIX}{}", STANDARD_NO_PAD.encode(bytes))
            )
            .is_err()
        );
        for bad in [
            "plain",
            "rustman:aes256gcm:v2:AAAA",
            "rustman:aes256gcm:v1:AA",
        ] {
            assert!(decrypt(&key, &context, bad).is_err());
        }
    }
}
