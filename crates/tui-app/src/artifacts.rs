//! Blob artifacts: lampiran/biner sebagai artefak dengan id stabil.
//!
//! Berkas disalin ke `<data>/artifacts/`, metadata JSONL menyimpan hash
//! SHA-256, nama asli, ukuran, dan path. Tidak ada konten yang dicetak ke chat.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Satu artefak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub nama: String,
    pub size: u64,
    pub sha256: String,
    pub file: String,
}

/// Folder artefak di bawah root data.
pub fn dir(root: &Path) -> PathBuf {
    root.join("artifacts")
}

/// Path indeks metadata.
pub fn index(root: &Path) -> PathBuf {
    root.join("artifacts.jsonl")
}

/// Muat indeks; baris rusak diabaikan.
pub fn daftar(root: &Path) -> Vec<Artifact> {
    let Ok(isi) = std::fs::read_to_string(index(root)) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|l| serde_json::from_str::<Artifact>(l).ok())
        .collect()
}

/// Simpan berkas sebagai artefak baru; mengembalikan metadata.
pub fn tambah(root: &Path, sumber: &Path) -> Result<Artifact, String> {
    let isi = std::fs::read(sumber).map_err(|e| format!("gagal baca {}: {e}", sumber.display()))?;
    let hash = format!("{:x}", Sha256::digest(&isi));
    let id = hash[..12].to_string();
    let ekst = sumber
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let nama_file = format!("{id}{ekst}");
    let folder = dir(root);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join(&nama_file), &isi).map_err(|e| e.to_string())?;
    let item = Artifact {
        id: id.clone(),
        nama: sumber
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| sumber.display().to_string()),
        size: isi.len() as u64,
        sha256: hash,
        file: nama_file,
    };
    let mut semua: Vec<Artifact> = daftar(root).into_iter().filter(|a| a.id != id).collect();
    semua.push(item.clone());
    let mut isi = String::new();
    for a in &semua {
        isi.push_str(&serde_json::to_string(a).map_err(|e| e.to_string())?);
        isi.push('\n');
    }
    std::fs::write(index(root), isi).map_err(|e| e.to_string())?;
    Ok(item)
}

/// Cari artefak berdasarkan id/awalan id.
pub fn cari(root: &Path, id: &str) -> Option<Artifact> {
    daftar(root)
        .into_iter()
        .find(|a| a.id == id || a.id.starts_with(id))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn tambah_dan_cari_artefak() {
        let root = std::env::temp_dir().join(format!(
            "maicli-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let sumber = root.join("input.bin");
        std::fs::write(&sumber, [0u8, 1, 2, 3]).unwrap();
        let item = tambah(&root, &sumber).unwrap();
        assert_eq!(item.size, 4);
        assert_eq!(item.sha256.len(), 64);
        assert_eq!(cari(&root, &item.id).unwrap().nama, "input.bin");
        assert_eq!(
            std::fs::read(dir(&root).join(&item.file)).unwrap(),
            vec![0u8, 1, 2, 3]
        );
        std::fs::remove_dir_all(root).ok();
    }
}
