//! Backend penyimpanan memori: JSONL, Markdown, dan SQLite.

use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tui_wire::{ErrorCode, MxError};

use crate::MemoryEntry;

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::SessionCorrupt, pesan.into())
}

/// Kontrak satu backend memori.
pub trait MemoryStore: Send {
    /// Nama backend untuk ditampilkan.
    fn backend(&self) -> &'static str;
    /// Simpan satu entry.
    fn add(&mut self, entry: &MemoryEntry) -> Result<MemoryEntry, MxError>;
    /// Semua entry, urut waktu dibuat.
    fn all(&self) -> Result<Vec<MemoryEntry>, MxError>;
    /// Hapus satu id; `Ok(false)` bila tidak ada.
    fn forget(&mut self, id: &str) -> Result<bool, MxError>;
    /// Pertahankan `max` entry terbaru; mengembalikan jumlah yang dipangkas.
    fn prune(&mut self, max: usize) -> Result<usize, MxError>;
}

fn tulis_atomik(path: &Path, isi: &str) -> Result<(), MxError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| galat(format!("{}: {e}", parent.display())))?;
    }
    let tmp = path.with_extension("tmp");
    let mut file =
        std::fs::File::create(&tmp).map_err(|e| galat(format!("{}: {e}", tmp.display())))?;
    file.write_all(isi.as_bytes())
        .map_err(|e| galat(format!("{}: {e}", tmp.display())))?;
    drop(file);
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    let _ = tui_wire::secure_file::restrict_to_current_user(path);
    Ok(())
}

// ---------------------------------------------------------------------------
// JSONL
// ---------------------------------------------------------------------------

/// Backend `notes.jsonl` (satu entry per baris).
pub struct JsonlStore {
    path: PathBuf,
    entries: Vec<MemoryEntry>,
}

impl JsonlStore {
    /// Buka/buat file JSONL.
    pub fn open(path: PathBuf) -> Result<Self, MxError> {
        let mut entries = Vec::new();
        match std::fs::read_to_string(&path) {
            Ok(isi) => {
                for (index, baris) in isi.lines().enumerate() {
                    if baris.trim().is_empty() {
                        continue;
                    }
                    let entry: MemoryEntry = serde_json::from_str(baris).map_err(|e| {
                        galat(format!(
                            "{}: baris {} rusak: {e}",
                            path.display(),
                            index + 1
                        ))
                    })?;
                    entries.push(entry);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(galat(format!("{}: {err}", path.display()))),
        }
        Ok(Self { path, entries })
    }

    fn simpan(&self) -> Result<(), MxError> {
        let mut isi = String::new();
        for entry in &self.entries {
            let baris = serde_json::to_string(entry).map_err(|e| galat(e.to_string()))?;
            isi.push_str(&baris);
            isi.push('\n');
        }
        tulis_atomik(&self.path, &isi)
    }
}

impl MemoryStore for JsonlStore {
    fn backend(&self) -> &'static str {
        "jsonl"
    }

    fn add(&mut self, entry: &MemoryEntry) -> Result<MemoryEntry, MxError> {
        self.entries.push(entry.clone());
        self.simpan()?;
        Ok(entry.clone())
    }

    fn all(&self) -> Result<Vec<MemoryEntry>, MxError> {
        Ok(self.entries.clone())
    }

    fn forget(&mut self, id: &str) -> Result<bool, MxError> {
        let sebelum = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == sebelum {
            return Ok(false);
        }
        self.simpan()?;
        Ok(true)
    }

    fn prune(&mut self, max: usize) -> Result<usize, MxError> {
        if self.entries.len() <= max {
            return Ok(0);
        }
        self.entries.sort_by_key(|entry| entry.created_ms);
        let buang = self.entries.len() - max;
        self.entries.drain(0..buang);
        self.simpan()?;
        Ok(buang)
    }
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/// Backend `MEMORY.md` (satu bagian per entry, tetap terbaca manusia).
pub struct MarkdownStore {
    path: PathBuf,
    entries: Vec<MemoryEntry>,
}

const MARKDOWN_HEADER: &str = "# MEMORY\n\n";

impl MarkdownStore {
    /// Buka/buat `MEMORY.md`.
    pub fn open(path: PathBuf) -> Result<Self, MxError> {
        let entries = match std::fs::read_to_string(&path) {
            Ok(isi) => parse_markdown(&path, &isi)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(galat(format!("{}: {err}", path.display()))),
        };
        Ok(Self { path, entries })
    }

    fn simpan(&self) -> Result<(), MxError> {
        let mut isi = String::from(MARKDOWN_HEADER);
        for entry in &self.entries {
            isi.push_str(&format!("## {}\n", entry.id));
            isi.push_str(&format!(
                "<!-- created_ms={} tags={} -->\n",
                entry.created_ms,
                entry.tags.join(",")
            ));
            isi.push_str(entry.text.trim());
            isi.push_str("\n\n");
        }
        tulis_atomik(&self.path, &isi)
    }
}

fn parse_markdown(path: &Path, isi: &str) -> Result<Vec<MemoryEntry>, MxError> {
    let mut entries = Vec::new();
    for bagian in isi.split("\n## ").skip(1) {
        let mut baris = bagian.lines();
        let Some(id) = baris.next().map(str::trim) else {
            continue;
        };
        let mut created_ms = 0;
        let mut tags = Vec::new();
        let mut sisa = Vec::new();
        for line in baris {
            let line = line.trim();
            if let Some(meta) = line
                .strip_prefix("<!--")
                .and_then(|v| v.strip_suffix("-->"))
            {
                for field in meta.split_whitespace() {
                    if let Some(v) = field.strip_prefix("created_ms=") {
                        created_ms = v.parse().unwrap_or(0);
                    } else if let Some(v) = field.strip_prefix("tags=") {
                        tags = v
                            .split(',')
                            .filter(|tag| !tag.is_empty())
                            .map(str::to_string)
                            .collect();
                    }
                }
                continue;
            }
            if !line.is_empty() || !sisa.is_empty() {
                sisa.push(line.to_string());
            }
        }
        let text = sisa.join("\n").trim().to_string();
        if text.is_empty() {
            continue;
        }
        entries.push(MemoryEntry {
            id: id.to_string(),
            text,
            tags,
            created_ms,
            embedding: None,
        });
    }
    let sisa = isi.trim_start_matches(MARKDOWN_HEADER.trim()).trim();
    if entries.is_empty() && !sisa.is_empty() && !isi.contains("## ") {
        return Err(galat(format!(
            "{}: format MEMORY.md tidak dikenal",
            path.display()
        )));
    }
    Ok(entries)
}

impl MemoryStore for MarkdownStore {
    fn backend(&self) -> &'static str {
        "markdown"
    }

    fn add(&mut self, entry: &MemoryEntry) -> Result<MemoryEntry, MxError> {
        self.entries.push(entry.clone());
        self.simpan()?;
        Ok(entry.clone())
    }

    fn all(&self) -> Result<Vec<MemoryEntry>, MxError> {
        Ok(self.entries.clone())
    }

    fn forget(&mut self, id: &str) -> Result<bool, MxError> {
        let sebelum = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == sebelum {
            return Ok(false);
        }
        self.simpan()?;
        Ok(true)
    }

    fn prune(&mut self, max: usize) -> Result<usize, MxError> {
        if self.entries.len() <= max {
            return Ok(0);
        }
        self.entries.sort_by_key(|entry| entry.created_ms);
        let buang = self.entries.len() - max;
        self.entries.drain(0..buang);
        self.simpan()?;
        Ok(buang)
    }
}

// ---------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------

/// Backend `memory.db` (SQLite, dependensi `rusqlite` bundled).
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Buka/buat database dan tabel `memory`.
    pub fn open(path: PathBuf) -> Result<Self, MxError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| galat(format!("{}: {e}", parent.display())))?;
        }
        let conn =
            Connection::open(&path).map_err(|e| galat(format!("{}: {e}", path.display())))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                created_ms INTEGER NOT NULL,
                embedding BLOB
            );",
        )
        .map_err(|e| galat(format!("sqlite: {e}")))?;
        let _ = tui_wire::secure_file::restrict_to_current_user(&path);
        Ok(Self { conn })
    }
}

fn embed_blob(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn blob_embed(blob: Vec<u8>) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(bytes)
        })
        .collect()
}

impl MemoryStore for SqliteStore {
    fn backend(&self) -> &'static str {
        "sqlite"
    }

    fn add(&mut self, entry: &MemoryEntry) -> Result<MemoryEntry, MxError> {
        let tags = entry.tags.join(",");
        let blob = entry.embedding.as_deref().map(embed_blob);
        self.conn
            .execute(
                "INSERT OR REPLACE INTO memory(id, text, tags, created_ms, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.id, entry.text, tags, entry.created_ms as i64, blob],
            )
            .map_err(|e| galat(format!("sqlite: {e}")))?;
        Ok(entry.clone())
    }

    fn all(&self) -> Result<Vec<MemoryEntry>, MxError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, text, tags, created_ms, embedding FROM memory ORDER BY created_ms")
            .map_err(|e| galat(format!("sqlite: {e}")))?;
        let rows = statement
            .query_map([], |row| {
                let tags: String = row.get(2)?;
                let blob: Option<Vec<u8>> = row.get(4)?;
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    tags: tags
                        .split(',')
                        .filter(|tag| !tag.is_empty())
                        .map(str::to_string)
                        .collect(),
                    created_ms: row.get::<_, i64>(3)? as u64,
                    embedding: blob.map(blob_embed),
                })
            })
            .map_err(|e| galat(format!("sqlite: {e}")))?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| galat(format!("sqlite: {e}")))?);
        }
        Ok(entries)
    }

    fn forget(&mut self, id: &str) -> Result<bool, MxError> {
        let removed = self
            .conn
            .execute("DELETE FROM memory WHERE id = ?1", params![id])
            .map_err(|e| galat(format!("sqlite: {e}")))?;
        Ok(removed > 0)
    }

    fn prune(&mut self, max: usize) -> Result<usize, MxError> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM memory WHERE id NOT IN (
                    SELECT id FROM memory ORDER BY created_ms DESC LIMIT ?1
                )",
                params![max as i64],
            )
            .map_err(|e| galat(format!("sqlite: {e}")))?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn entry(id: &str, text: &str, created_ms: u64) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            text: text.to_string(),
            tags: vec!["uji".to_string()],
            created_ms,
            embedding: None,
        }
    }

    #[test]
    fn markdown_bolak_balik() {
        let dir = std::env::temp_dir().join(format!("maicli-md-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("MEMORY.md");
        let mut store = MarkdownStore::open(path.clone()).unwrap();
        store.add(&entry("a", "baris satu\nbaris dua", 1)).unwrap();
        let store = MarkdownStore::open(path).unwrap();
        let entries = store.all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "baris satu\nbaris dua");
        assert_eq!(entries[0].tags, ["uji"]);
    }

    #[test]
    fn sqlite_forget_dan_prune() {
        let dir = std::env::temp_dir().join(format!("maicli-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = SqliteStore::open(dir.join("memory.db")).unwrap();
        store.add(&entry("a", "satu", 1)).unwrap();
        store.add(&entry("b", "dua", 2)).unwrap();
        assert!(store.forget("a").unwrap());
        assert!(!store.forget("a").unwrap());
        assert_eq!(store.all().unwrap().len(), 1);
        store.add(&entry("c", "tiga", 3)).unwrap();
        assert_eq!(store.prune(1).unwrap(), 1);
        assert_eq!(store.all().unwrap()[0].id, "c");
    }
}
