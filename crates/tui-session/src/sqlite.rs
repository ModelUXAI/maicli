//! Indeks sesi SQLite (additive).
//!
//! File JSONL lama di `~/.config/ModeluXAI/TUI/sessions/` tidak diubah dan tetap
//! bisa dibaca/di-resume. Modul ini hanya menambahkan indeks cepat + salinan
//! transkrip agar sesi bisa di-backup tanpa kehilangan percakapan.
//!
//! Layout baru untuk sesi yang dibuat setelah modul ini aktif:
//! `.maicli/sessions/<uuid>/isi-sessions/<file>.jsonl`
//! dengan UUID v4 (bukan hex+sequence).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use tui_wire::{ErrorCode, MxError};

/// Nama berkas indeks di akar direktori sesi.
pub const INDEX_NAME: &str = "index.sqlite3";

/// Satu baris indeks sesi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSession {
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub updated_at: u64,
}

fn sql_error(err: rusqlite::Error) -> MxError {
    MxError::new(
        ErrorCode::SessionCorrupt,
        format!("indeks sesi SQLite gagal: {err}"),
    )
}

/// UUID v4 untuk id sesi baru (kompatibel dengan contoh `omp --resume <uuid>`).
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Direktori sesi baru: `<root>/sessions/<uuid>/isi-sessions`.
pub fn session_dir(root: &Path, id: &str) -> PathBuf {
    root.join("sessions").join(id).join("isi-sessions")
}

/// Path indeks: `<root>/sessions/index.sqlite3`.
pub fn index_path(root: &Path) -> PathBuf {
    root.join("sessions").join(INDEX_NAME)
}

/// Buka (atau buat) indeks dan skemanya.
pub fn open(root: &Path) -> Result<Connection, MxError> {
    let path = index_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            MxError::new(
                ErrorCode::SessionCorrupt,
                format!("gagal membuat folder sesi: {err}"),
            )
        })?;
    }
    let conn = Connection::open(&path).map_err(sql_error)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS sessions (
             id TEXT PRIMARY KEY,
             path TEXT NOT NULL,
             title TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at DESC);",
    )
    .map_err(sql_error)?;
    Ok(conn)
}

/// Daftarkan / perbarui satu sesi pada indeks.
pub fn upsert(
    conn: &Connection,
    id: &str,
    path: &Path,
    title: &str,
    updated_at: u64,
) -> Result<(), MxError> {
    conn.execute(
        "INSERT INTO sessions (id, path, title, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET path=excluded.path, title=excluded.title,
             updated_at=excluded.updated_at",
        params![id, path.to_string_lossy(), title, updated_at as i64],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// Ambil satu sesi dari indeks berdasarkan id (prefiks juga diterima).
pub fn resolve(conn: &Connection, requested: &str) -> Result<Option<IndexedSession>, MxError> {
    let exact = conn
        .query_row(
            "SELECT id, path, title, updated_at FROM sessions WHERE id = ?1",
            params![requested],
            |row| {
                Ok(IndexedSession {
                    id: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    title: row.get(2)?,
                    updated_at: row.get::<_, i64>(3)? as u64,
                })
            },
        )
        .optional()
        .map_err(sql_error)?;
    if exact.is_some() {
        return Ok(exact);
    }
    let pattern = format!("{requested}%");
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, updated_at FROM sessions WHERE id LIKE ?1
             ORDER BY updated_at DESC LIMIT 2",
        )
        .map_err(sql_error)?;
    let rows = stmt
        .query_map(params![pattern], |row| {
            Ok(IndexedSession {
                id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                title: row.get(2)?,
                updated_at: row.get::<_, i64>(3)? as u64,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    match rows.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only.clone())),
        _ => Err(MxError::new(
            ErrorCode::SessionNotFound,
            format!("awalan '{requested}' cocok dengan beberapa sesi; pakai id lebih panjang"),
        )),
    }
}

/// Semua sesi pada indeks, terbaru lebih dulu.
pub fn all(conn: &Connection) -> Result<Vec<IndexedSession>, MxError> {
    let mut stmt = conn
        .prepare("SELECT id, path, title, updated_at FROM sessions ORDER BY updated_at DESC")
        .map_err(sql_error)?;
    stmt.query_map([], |row| {
        Ok(IndexedSession {
            id: row.get(0)?,
            path: PathBuf::from(row.get::<_, String>(1)?),
            title: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
        })
    })
    .map_err(sql_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(sql_error)
}

/// Salin seluruh transkrip JSONL ke direktori backup yang dipilih pengguna.
/// Mengembalikan jumlah berkas yang disalin.
pub fn backup(conn: &Connection, destination: &Path) -> Result<usize, MxError> {
    std::fs::create_dir_all(destination).map_err(|err| {
        MxError::new(
            ErrorCode::SessionCorrupt,
            format!("gagal membuat folder backup: {err}"),
        )
    })?;
    let mut copied = 0usize;
    for session in all(conn)? {
        if !session.path.is_file() {
            continue;
        }
        let name = session
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.jsonl", session.id));
        let target = destination.join(name);
        std::fs::copy(&session.path, &target).map_err(|err| {
            MxError::new(
                ErrorCode::SessionCorrupt,
                format!("gagal menyalin sesi {}: {err}", session.id),
            )
        })?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("maicli-sqlite-{name}-{}", new_uuid()))
    }

    #[test]
    fn uuid_v4_dan_layout_folder() {
        let id = new_uuid();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().nth(14), Some('4'), "versi UUID harus 4");
        let dir = session_dir(Path::new("/root"), &id);
        assert!(dir.ends_with("isi-sessions"));
        assert!(dir.to_string_lossy().contains(&id));
    }

    #[test]
    fn indeks_menyimpan_dan_mencari_sesi() {
        let base = root("index");
        let conn = open(&base).expect("buka indeks");
        let path = session_dir(&base, "abc").join("sesi.jsonl");
        upsert(&conn, "abc", &path, "Judul uji", 42).expect("upsert");
        let found = resolve(&conn, "abc").expect("resolve").expect("ada");
        assert_eq!(found.title, "Judul uji");
        assert_eq!(found.path, path);
        assert!(resolve(&conn, "ab").expect("prefiks").is_some());
        assert!(resolve(&conn, "zzz").expect("tidak ada").is_none());
        assert_eq!(all(&conn).expect("all").len(), 1);
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn backup_menyalin_transkrip() {
        let base = root("backup");
        let conn = open(&base).expect("buka indeks");
        let dir = session_dir(&base, "s1");
        std::fs::create_dir_all(&dir).expect("buat folder");
        let src = dir.join("sesi.jsonl");
        std::fs::write(&src, "{\"type\":\"session\"}\n").expect("tulis");
        upsert(&conn, "s1", &src, "Satu", 1).expect("upsert");
        let dest = base.join("backup");
        assert_eq!(backup(&conn, &dest).expect("backup"), 1);
        assert!(dest.join("sesi.jsonl").is_file());
        std::fs::remove_dir_all(base).ok();
    }
}
