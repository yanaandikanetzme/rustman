//! Mock server definitions, one JSON row each.
use crate::domain::mock::MockServer;
use rusqlite::{Connection, params};

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mock_servers (
        id TEXT PRIMARY KEY,
        data TEXT NOT NULL
    );",
    )
}

pub fn load(conn: &Connection) -> Result<Vec<MockServer>, String> {
    let mut stmt = conn
        .prepare("SELECT data FROM mock_servers ORDER BY rowid")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    // A row from a newer version that no longer parses is skipped, not fatal.
    Ok(rows
        .filter_map(|r| r.ok())
        .filter_map(|data| serde_json::from_str(&data).ok())
        .collect())
}

pub fn save(conn: &Connection, mock: &MockServer) -> Result<(), String> {
    let data = serde_json::to_string(mock).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO mock_servers (id, data) VALUES (?1, ?2)
        ON CONFLICT(id) DO UPDATE SET data=excluded.data",
        params![mock.id, data],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM mock_servers WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocks_round_trip() {
        let dir = std::env::temp_dir().join(format!("rustman-mock-{}", uuid::Uuid::new_v4()));
        let db = super::super::open(&dir).unwrap();
        let mut mock = MockServer {
            id: "m1".into(),
            name: "Merchant mock".into(),
            collection_id: "c".into(),
            environment_id: None,
            port: 4010,
            delay_ms: 0,
            auto_start: false,
            created_at: 1,
        };
        save(&db, &mock).unwrap();
        mock.port = 4011;
        save(&db, &mock).unwrap();
        assert_eq!(load(&db).unwrap(), vec![mock]);
        delete(&db, "m1").unwrap();
        assert!(load(&db).unwrap().is_empty());
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
