use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub fn open(path: &str) -> Result<Connection> {
    let expanded = path.replacen("~", &std::env::var("HOME").unwrap_or_default(), 1);
    let conn = Connection::open(&expanded)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hashes (
            path TEXT PRIMARY KEY,
            mtime INTEGER NOT NULL,
            hashes BLOB NOT NULL,
            audio_hash INTEGER NOT NULL,
            duration REAL NOT NULL
        );",
    )?;
    Ok(conn)
}

pub fn get(conn: &Connection, path: &Path, mtime: u64) -> Option<(Vec<u64>, u64, f64)> {
    conn.query_row(
        "SELECT hashes, audio_hash, duration FROM hashes WHERE path=?1 AND mtime=?2",
        params![path.to_str()?, mtime as i64],
        |row| {
            let blob: Vec<u8> = row.get(0)?;
            let audio: i64 = row.get(1)?;
            let dur: f64 = row.get(2)?;
            Ok((blob, audio as u64, dur))
        },
    )
    .ok()
    .map(|(blob, audio, dur)| {
        let hashes = blob
            .chunks_exact(8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        (hashes, audio, dur)
    })
}

pub fn put(conn: &Connection, path: &Path, mtime: u64, hashes: &[u64], audio: u64, dur: f64) {
    let blob: Vec<u8> = hashes.iter().flat_map(|h| h.to_le_bytes()).collect();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO hashes (path, mtime, hashes, audio_hash, duration) \
         VALUES (?1,?2,?3,?4,?5)",
        params![path.to_str().unwrap_or(""), mtime as i64, blob, audio as i64, dur],
    );
}
