//! Catatan konteks persisten per proyek (omp `context_notes`).
//!
//! Disimpan sebagai JSONL di `<data_dir>/context-notes.jsonl`, dimuat saat boot,
//! lalu disuntikkan ke system prompt. `/new-context` memulai sesi baru tanpa
//! menghapus catatan, sehingga pengetahuan manual tetap bertahan lintas sesi.

use serde::{Deserialize, Serialize};

/// Satu catatan konteks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Teks catatan.
    pub text: String,
    /// Waktu pencatatan (Unix ms; 0 bila dari berkas lama).
    #[serde(default)]
    pub ts: u64,
}

/// Path penyimpanan catatan.
pub fn path() -> std::path::PathBuf {
    crate::config_root().join("context-notes.jsonl")
}

/// Path anotasi hasil `/review` (dipisah dari catatan manual).
pub fn annotation_path() -> std::path::PathBuf {
    crate::config_root().join("annotations.jsonl")
}

/// Path notifikasi pengguna (`/notifications`).
pub fn notification_path() -> std::path::PathBuf {
    crate::config_root().join("notifications.jsonl")
}

/// Muat semua catatan; baris rusak diabaikan (file besifat lokal & opsional).
pub fn muat() -> Vec<Note> {
    let Ok(isi) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|baris| serde_json::from_str::<Note>(baris).ok())
        .filter(|n| !n.text.trim().is_empty())
        .collect()
}

/// Tambah satu catatan; mengembalikan jumlah catatan setelah penulisan.
pub fn tambah(teks: &str) -> Result<usize, String> {
    let teks = teks.trim();
    if teks.is_empty() {
        return Err("catatan tidak boleh kosong".to_string());
    }
    let mut semua = muat();
    let cap = path();
    if let Some(induk) = cap.parent() {
        let _ = std::fs::create_dir_all(induk);
    }
    let note = Note {
        text: teks.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    semua.push(note);
    simpan(&semua)?;
    Ok(semua.len())
}

/// Hapus semua catatan.
pub fn bersihkan() -> Result<(), String> {
    simpan(&[])
}

/// Tulis ulang seluruh daftar secara atomik (temporary + rename).
fn simpan(semua: &[Note]) -> Result<(), String> {
    let target = path();
    let tmp = target.with_extension("jsonl.tmp");
    let mut isi = String::new();
    for note in semua {
        isi.push_str(&serde_json::to_string(note).map_err(|e| e.to_string())?);
        isi.push('\n');
    }
    std::fs::write(&tmp, isi).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// Muat anotasi review.
pub fn annotation_muat() -> Vec<Note> {
    let Ok(isi) = std::fs::read_to_string(annotation_path()) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|baris| serde_json::from_str::<Note>(baris).ok())
        .filter(|n| !n.text.trim().is_empty())
        .collect()
}

/// Tambah anotasi review, mengembalikan jumlah anotasi.
pub fn annotation_tambah(teks: &str) -> Result<usize, String> {
    let teks = teks.trim();
    if teks.is_empty() {
        return Err("anotasi tidak boleh kosong".to_string());
    }
    let mut semua = annotation_muat();
    semua.push(Note {
        text: teks.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    });
    tulis(&annotation_path(), &semua)?;
    Ok(semua.len())
}

/// Hapus semua anotasi review.
pub fn annotation_bersihkan() -> Result<(), String> {
    tulis(&annotation_path(), &[])
}

/// Muat notifikasi pengguna.
pub fn notification_muat() -> Vec<Note> {
    let Ok(isi) = std::fs::read_to_string(notification_path()) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|baris| serde_json::from_str::<Note>(baris).ok())
        .filter(|n| !n.text.trim().is_empty())
        .collect()
}

/// Tambah notifikasi; mengembalikan jumlah yang tersimpan.
pub fn notification_tambah(teks: &str) -> Result<usize, String> {
    let teks = teks.trim();
    if teks.is_empty() {
        return Err("notifikasi tidak boleh kosong".to_string());
    }
    let mut semua = notification_muat();
    semua.push(Note {
        text: teks.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    });
    tulis(&notification_path(), &semua)?;
    Ok(semua.len())
}

/// Hapus semua notifikasi.
pub fn notification_bersihkan() -> Result<(), String> {
    tulis(&notification_path(), &[])
}

/// Blok system prompt untuk notifikasi aktif.
pub fn notification_blok(semua: &[Note]) -> String {
    if semua.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Notifikasi aktif (/notifications)\n");
    for note in semua {
        out.push_str(&format!("- {}\n", note.text.replace('\n', " ")));
    }
    out
}

/// Tulis daftar note ke path tertentu secara atomik.
fn tulis(target: &std::path::Path, semua: &[Note]) -> Result<(), String> {
    let tmp = target.with_extension("jsonl.tmp");
    let mut isi = String::new();
    for note in semua {
        isi.push_str(&serde_json::to_string(note).map_err(|e| e.to_string())?);
        isi.push('\n');
    }
    std::fs::write(&tmp, isi).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())
}

/// Blok system prompt untuk anotasi review.
pub fn annotation_blok(semua: &[Note]) -> String {
    if semua.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Anotasi review (/review, /annotate)\n");
    for note in semua {
        out.push_str(&format!("- {}\n", note.text.replace('\n', " ")));
    }
    out
}

/// Render blok system prompt; kosong bila tidak ada catatan.
pub fn blok(semua: &[Note]) -> String {
    if semua.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Catatan konteks proyek (dari /context-note)\n");
    for note in semua {
        out.push_str(&format!("- {}\n", note.text.replace('\n', " ")));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn blok_berisi_catatan() {
        let notes = vec![Note {
            text: " pakai API v2 ".to_string(),
            ts: 1,
        }];
        assert!(blok(&notes).contains("pakai API v2"));
        assert!(blok(&[]).is_empty());
    }

    #[test]
    fn parse_baris_rusak_diabaikan() {
        let baris = "{\"text\":\"baik\"}\nbaris-rusak\n";
        let hasil: Vec<Note> = baris
            .lines()
            .filter_map(|l| serde_json::from_str::<Note>(l).ok())
            .collect();
        assert_eq!(hasil.len(), 1);
        assert_eq!(hasil[0].text, "baik");
    }
}

#[cfg(test)]
mod annotation_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn annotation_blok_berisi_verdict() {
        let n = vec![Note {
            text: "P1: cek error handling".to_string(),
            ts: 1,
        }];
        assert!(annotation_blok(&n).contains("P1: cek error handling"));
        assert!(annotation_blok(&[]).is_empty());
    }
}

#[cfg(test)]
mod notification_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn notification_blok_berisi_pesan() {
        let n = vec![Note {
            text: "gateway maintenance".to_string(),
            ts: 1,
        }];
        assert!(notification_blok(&n).contains("gateway maintenance"));
        assert!(notification_blok(&[]).is_empty());
    }
}
