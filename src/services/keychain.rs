//! Native credential-store boundary. Database files never contain the key.

#[cfg(not(test))]
pub fn key(id: &str, create: bool) -> Result<Vec<u8>, String> {
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd"
    )))]
    return Err("No supported native credential store on this platform".into());
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    {
        let entry = keyring::Entry::new("org.rustman.database.v1", id)
            .map_err(|_| "Could not open OS credential store".to_owned())?;
        match entry.get_secret() {
            Ok(key) if key.len() == 32 => Ok(key),
            Ok(_) => Err("Database encryption key has an invalid length".into()),
            Err(keyring::Error::NoEntry) if create => {
                use aes_gcm::aead::{KeyInit, OsRng};
                let key = aes_gcm::Aes256Gcm::generate_key(OsRng).to_vec();
                entry.set_secret(&key).map_err(|_| "Could not save database key in OS credential store".to_owned())?;
                // Read back: never persist encrypted data against an unconfirmed key.
                let stored = entry.get_secret().map_err(|_| "Could not verify database key in OS credential store".to_owned())?;
                if stored != key { return Err("OS credential store key verification failed".into()); }
                Ok(key)
            }
            Err(keyring::Error::NoEntry) => Err("Database encryption key is missing; restore the original OS credential. No data was overwritten".into()),
            Err(_) => Err("OS credential store is locked or unavailable; no plaintext fallback is permitted".into()),
        }
    }
}

// Unit tests use only ephemeral keys, never a user's credential store.
#[cfg(test)]
pub fn key(id: &str, create: bool) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{KeyInit, OsRng};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static KEYS: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    let mut keys = KEYS.get_or_init(Default::default).lock().unwrap();
    if let Some(key) = keys.get(id) {
        return Ok(key.clone());
    }
    if !create {
        return Err("Database encryption key is missing".into());
    }
    let key = aes_gcm::Aes256Gcm::generate_key(OsRng).to_vec();
    keys.insert(id.into(), key.clone());
    Ok(key)
}
// Migration-complete state lives outside the attacker-editable SQLite file.
#[cfg(not(test))]
pub fn migration_complete(id: &str) -> Result<bool, String> {
    let entry = keyring::Entry::new("org.rustman.database.migration.v1", id)
        .map_err(|_| "Could not open OS migration state".to_owned())?;
    match entry.get_password() {
        Ok(value) if value == "encrypted-v1" => Ok(true),
        Ok(_) => Err("Unsupported trusted migration state".into()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err("OS migration state is locked or unavailable".into()),
    }
}

#[cfg(not(test))]
pub fn mark_migrated(id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("org.rustman.database.migration.v1", id)
        .map_err(|_| "Could not open OS migration state".to_owned())?;
    entry
        .set_password("encrypted-v1")
        .map_err(|_| "Could not store trusted migration state".to_owned())?;
    if !migration_complete(id)? {
        return Err("Could not verify trusted migration state".into());
    }
    Ok(())
}

#[cfg(test)]
fn migration_states() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static STATES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    STATES.get_or_init(Default::default)
}
#[cfg(test)]
pub fn migration_complete(id: &str) -> Result<bool, String> {
    key(id, false)?;
    Ok(migration_states().lock().unwrap().contains(id))
}
#[cfg(test)]
pub fn mark_migrated(id: &str) -> Result<(), String> {
    key(id, false)?;
    migration_states().lock().unwrap().insert(id.to_owned());
    Ok(())
}
