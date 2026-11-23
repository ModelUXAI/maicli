//! Memori persisten lintas sesi.
//!
//! Backend dapat dipilih pengguna: `jsonl` (`notes.jsonl`), `markdown`
//! (`MEMORY.md`), atau `sqlite` (`memory.db`). Pencarian memakai embedding
//! lokal deterministik (hash n-gram, tanpa model/jaringan) atau skor keyword
//! bila embedding dimatikan.
//!
//! Mental model disimpan sebagai berkas Markdown di `<memory>/mental-models/`
//! dan bisa di-refresh dari entry memori tanpa memanggil model.

mod backend;
mod embedding;
mod mental;
mod tool;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tui_wire::{ErrorCode, MxError};

pub use backend::MemoryStore;
pub use embedding::{DIM, cosine, embed};
pub use mental::MentalModel;
pub use tool::MemoryTool;

/// Satu entry memori.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEntry {
    /// Id unik.
    pub id: String,
    /// Isi memori.
    pub text: String,
    /// Tag bebas.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Waktu dibuat (ms sejak epoch).
    pub created_ms: u64,
    /// Embedding lokal; `None` bila backend tidak menyimpannya.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

/// Satu hasil pencarian.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryHit {
    /// Entry yang cocok.
    pub entry: MemoryEntry,
    /// Skor kemiripan (makin besar makin cocok).
    pub score: f32,
}

/// Statistik memori untuk `/memory stats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    /// Nama backend.
    pub backend: String,
    /// Jumlah entry.
    pub entries: usize,
    /// Folder memori.
    pub dir: PathBuf,
    /// Apakah pencarian memakai embedding.
    pub embedding: bool,
}

/// Pegangan memori satu proyek.
pub struct MemoryHandle {
    dir: PathBuf,
    max_entries: usize,
    inject_chars: usize,
    inject_models: bool,
    embedding: bool,
    store: Mutex<Box<dyn MemoryStore>>,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::Config, pesan.into())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_id() -> String {
    // Urutan atomik mencegah dua entry dalam milidetik yang sama bertabrakan.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{seq:x}", now_ms())
}

impl MemoryHandle {
    /// Buka memori proyek.
    ///
    /// `backend` = `jsonl`/`markdown`/`sqlite`; folder dibuat bila belum ada.
    pub fn open(
        dir: &Path,
        backend: &str,
        embedding: bool,
        max_entries: usize,
        inject_chars: usize,
        inject_models: bool,
    ) -> Result<Self, MxError> {
        std::fs::create_dir_all(dir).map_err(|e| galat(format!("{}: {e}", dir.display())))?;
        let store: Box<dyn MemoryStore> = match backend {
            "jsonl" => Box::new(backend::JsonlStore::open(dir.join("notes.jsonl"))?),
            "markdown" => Box::new(backend::MarkdownStore::open(dir.join("MEMORY.md"))?),
            "sqlite" => Box::new(backend::SqliteStore::open(dir.join("memory.db"))?),
            lain => {
                return Err(galat(format!(
                    "backend memori '{lain}' tidak dikenal (jsonl|markdown|sqlite)"
                )));
            }
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            max_entries: max_entries.max(1),
            inject_chars,
            inject_models,
            embedding,
            store: Mutex::new(store),
        })
    }

    /// Nama backend aktif.
    pub fn backend(&self) -> String {
        self.store
            .lock()
            .map(|store| store.backend().to_string())
            .unwrap_or_else(|_| "tidak diketahui".to_string())
    }

    /// Folder memori.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Box<dyn MemoryStore>>, MxError> {
        self.store.lock().map_err(|_| galat("kunci memori rusak"))
    }

    /// Tambah satu entry baru.
    pub fn add(&self, text: &str, tags: &[String]) -> Result<MemoryEntry, MxError> {
        let teks = text.trim();
        if teks.is_empty() {
            return Err(galat("isi memori tidak boleh kosong"));
        }
        let mut entry = MemoryEntry {
            id: new_id(),
            text: teks.to_string(),
            tags: tags.to_vec(),
            created_ms: now_ms(),
            embedding: None,
        };
        if self.embedding {
            entry.embedding = Some(embedding::embed(teks));
        }
        let mut store = self.lock()?;
        let saved = store.add(&entry)?;
        let _ = store.prune(self.max_entries)?;
        Ok(saved)
    }

    /// Semua entry (urut waktu dibuat).
    pub fn all(&self) -> Result<Vec<MemoryEntry>, MxError> {
        self.lock()?.all()
    }

    /// Entry terbaru, terbaru lebih dulu.
    pub fn recent(&self, limit: usize) -> Result<Vec<MemoryEntry>, MxError> {
        let mut entries = self.all()?;
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_ms));
        entries.truncate(limit);
        Ok(entries)
    }

    /// Cari entry yang relevan.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, MxError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let entries = self.all()?;
        let query_vector = if self.embedding {
            Some(embedding::embed(query))
        } else {
            None
        };
        let mut hits: Vec<MemoryHit> = entries
            .into_iter()
            .filter_map(|entry| {
                let score = match (&query_vector, &entry.embedding) {
                    (Some(q), Some(e)) => embedding::cosine(q, e),
                    // Backend Markdown tidak menyimpan vektor; hitung saat dipakai.
                    (Some(q), None) => embedding::cosine(q, &embedding::embed(&entry.text)),
                    _ => keyword_score(query, &entry.text),
                };
                (score > 0.0).then_some(MemoryHit { entry, score })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.entry.created_ms.cmp(&a.entry.created_ms))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    /// Hapus satu entry berdasarkan id.
    pub fn forget(&self, id: &str) -> Result<bool, MxError> {
        self.lock()?.forget(id)
    }

    /// Statistik ringkas.
    pub fn stats(&self) -> Result<MemoryStats, MxError> {
        let store = self.lock()?;
        Ok(MemoryStats {
            backend: store.backend().to_string(),
            entries: store.all()?.len(),
            dir: self.dir.clone(),
            embedding: self.embedding,
        })
    }

    /// Blok memori untuk system prompt (entry terbaru + mental model).
    pub fn prompt_block(&self) -> Result<String, MxError> {
        let entries = self.recent(12)?;
        if entries.is_empty() && !self.inject_models {
            return Ok(String::new());
        }
        let mut out = String::from("\n\nMEMORI PROYEK (persisten, lintas sesi):\n");
        let mut terpakai = 0usize;
        for entry in &entries {
            let baris = if entry.tags.is_empty() {
                format!("- {}\n", entry.text)
            } else {
                format!("- [{}] {}\n", entry.tags.join(", "), entry.text)
            };
            if terpakai + baris.chars().count() > self.inject_chars {
                break;
            }
            terpakai += baris.chars().count();
            out.push_str(&baris);
        }
        if self.inject_models {
            let models = self.list_models()?;
            if !models.is_empty() {
                out.push_str("\nMENTAL MODEL AKTIF:\n");
                for model in models.iter().take(8) {
                    out.push_str(&format!("- {}: {}\n", model.name, model.ringkas));
                }
            }
        }
        Ok(out)
    }

    /// Daftar mental model.
    pub fn list_models(&self) -> Result<Vec<MentalModel>, MxError> {
        mental::list(&self.dir)
    }

    /// Baca satu mental model.
    pub fn read_model(&self, name: &str) -> Result<MentalModel, MxError> {
        mental::read(&self.dir, name)
    }

    /// Tulis satu mental model.
    pub fn write_model(&self, name: &str, isi: &str) -> Result<MentalModel, MxError> {
        mental::write(&self.dir, name, isi)
    }

    /// Refresh mental model dari entry memori terbaru.
    pub fn refresh_model(&self, name: &str) -> Result<MentalModel, MxError> {
        let entries = self.recent(20)?;
        if entries.is_empty() {
            return Err(galat("belum ada memori untuk dijadikan mental model"));
        }
        let mut isi = format!(
            "Mental model '{}' di-refresh dari {} entry memori.\n\n",
            name,
            entries.len()
        );
        for entry in entries {
            isi.push_str(&format!("- {}\n", entry.text));
        }
        mental::write(&self.dir, name, &isi)
    }

    /// Hapus satu mental model.
    pub fn remove_model(&self, name: &str) -> Result<bool, MxError> {
        mental::remove(&self.dir, name)
    }

    /// Refresh semua mental model yang ada dari entry memori terbaru.
    pub fn refresh_all_models(&self) -> Result<usize, MxError> {
        let models = self.list_models()?;
        let mut jumlah = 0;
        for model in models {
            self.refresh_model(&model.name)?;
            jumlah += 1;
        }
        Ok(jumlah)
    }
}

/// Skor keyword sederhana: jumlah token query yang muncul di teks.
fn keyword_score(query: &str, text: &str) -> f32 {
    let teks = text.to_lowercase();
    let tokens = tokens(query);
    if tokens.is_empty() {
        return 0.0;
    }
    let mut cocok = 0usize;
    for token in &tokens {
        if teks.contains(token) {
            cocok += 1;
        }
    }
    let mut score = cocok as f32 / tokens.len() as f32;
    if teks.contains(&query.to_lowercase()) {
        score += 0.5;
    }
    score
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn temp_dir(nama: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maicli-memory-{nama}-{}-{:x}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn id_unik_dalam_proses() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }

    #[test]
    fn jsonl_tambah_cari_dan_lupakan() {
        let dir = temp_dir("jsonl");
        let memory = MemoryHandle::open(&dir, "jsonl", true, 10, 500, false).unwrap();
        let entry = memory
            .add("gateway memakai rustls", &["arsitektur".to_string()])
            .unwrap();
        let hits = memory.search("rustls", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.id, entry.id);
        assert!(memory.forget(&entry.id).unwrap());
        assert!(memory.search("rustls", 5).unwrap().is_empty());
    }

    #[test]
    fn markdown_tetap_terbaca_setelah_dibuka_ulang() {
        let dir = temp_dir("markdown");
        {
            let memory = MemoryHandle::open(&dir, "markdown", false, 10, 500, false).unwrap();
            memory.add("catatan penting", &[]).unwrap();
        }
        let memory = MemoryHandle::open(&dir, "markdown", false, 10, 500, false).unwrap();
        let entries = memory.all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "catatan penting");
    }

    #[test]
    fn sqlite_menyimpan_dan_memangkas() {
        let dir = temp_dir("sqlite");
        let memory = MemoryHandle::open(&dir, "sqlite", true, 2, 500, false).unwrap();
        memory.add("satu", &[]).unwrap();
        memory.add("dua", &[]).unwrap();
        memory.add("tiga", &[]).unwrap();
        let entries = memory.all().unwrap();
        assert_eq!(entries.len(), 2, "max_entries dipatuhi");
        assert!(entries.iter().all(|entry| entry.text != "satu"));
    }

    #[test]
    fn prompt_block_menghormati_batas_karakter() {
        let dir = temp_dir("prompt");
        let memory = MemoryHandle::open(&dir, "jsonl", false, 10, 40, false).unwrap();
        memory.add(&"x".repeat(100), &[]).unwrap();
        let block = memory.prompt_block().unwrap();
        assert!(block.chars().count() < 120, "{block}");
    }

    #[test]
    fn mental_model_refresh_dari_memori() {
        let dir = temp_dir("mm");
        let memory = MemoryHandle::open(&dir, "jsonl", false, 10, 500, true).unwrap();
        memory.add("keputusan: pakai SQLite", &[]).unwrap();
        let model = memory.refresh_model("arsitektur").unwrap();
        assert!(model.isi.contains("SQLite"));
        assert_eq!(memory.list_models().unwrap().len(), 1);
    }
}
