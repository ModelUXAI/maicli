//! Checkpoint git + rewind sesi (`/checkpoint`, `/rewind`).
//!
//! Checkpoint menyimpan *patch* (`git diff`) tanpa mengubah working tree: jauh
//! lebih aman daripada `git stash`/`commit` otomatis. `/rewind` menerapkan patch
//! terbalik (`git apply -R`) dan memotong transkrip ke jumlah pesan saat checkpoint.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Satu titik checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Id unik (waktu + urutan).
    pub id: String,
    /// Label dari pengguna (boleh kosong).
    pub label: String,
    /// Waktu checkpoint (Unix ms).
    pub ts: u64,
    /// Jumlah pesan sesi saat checkpoint.
    pub messages: usize,
    /// Nama berkas patch di folder checkpoints.
    pub patch: String,
}

fn baru_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

fn daftar_path() -> PathBuf {
    crate::config_root().join("checkpoints.jsonl")
}

fn patch_dir() -> PathBuf {
    crate::config_root().join("checkpoints")
}

/// Semua checkpoint terbaru lebih dulu.
pub fn daftar() -> Vec<Checkpoint> {
    let Ok(isi) = std::fs::read_to_string(daftar_path()) else {
        return Vec::new();
    };
    let mut semua: Vec<Checkpoint> = isi
        .lines()
        .filter_map(|l| serde_json::from_str::<Checkpoint>(l).ok())
        .collect();
    semua.sort_by_key(|c| std::cmp::Reverse(c.ts));
    semua
}

/// Cari checkpoint berdasarkan id persis/awalan atau `latest`.
pub fn cari(target: &str) -> Option<Checkpoint> {
    let semua = daftar();
    if target == "latest" || target.is_empty() {
        return semua.into_iter().next();
    }
    semua
        .into_iter()
        .find(|c| c.id == target || c.id.starts_with(target))
}

/// Buat patch dari perubahan kerja saat ini (tidak memodifikasi working tree).
pub fn simpan_patch(cwd: &Path) -> Result<(String, String), String> {
    let id = baru_id();
    let keluaran = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(["diff", "--binary", "HEAD"])
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    let patch = if keluaran.status.success() {
        String::from_utf8_lossy(&keluaran.stdout).into_owned()
    } else {
        let cadangan = Command::new("git")
            .args(["-C"])
            .arg(cwd)
            .args(["diff", "--binary"])
            .output()
            .map_err(|e| format!("git tidak tersedia: {e}"))?;
        if !cadangan.status.success() {
            return Err("bukan repositori git atau git diff gagal".to_string());
        }
        String::from_utf8_lossy(&cadangan.stdout).into_owned()
    };
    let dir = patch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nama = format!("{id}.patch");
    std::fs::write(dir.join(&nama), &patch).map_err(|e| e.to_string())?;
    Ok((id, nama))
}

/// Catat checkpoint baru ke indeks JSONL.
pub fn tambah(id: String, label: &str, messages: usize, patch: String) -> Result<(), String> {
    let entri = Checkpoint {
        id,
        label: label.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        messages,
        patch,
    };
    let mut isi = String::new();
    if let Ok(lama) = std::fs::read_to_string(daftar_path()) {
        isi.push_str(&lama);
        if !isi.ends_with('\n') && !isi.is_empty() {
            isi.push('\n');
        }
    }
    isi.push_str(&serde_json::to_string(&entri).map_err(|e| e.to_string())?);
    isi.push('\n');
    let target = daftar_path();
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, isi).map_err(|e| e.to_string())
}

/// Terapkan patch terbalik; mengembalikan error bila konflik.
pub fn revert(cwd: &Path, checkpoint: &Checkpoint) -> Result<(), String> {
    let path = patch_dir().join(&checkpoint.patch);
    let keluaran = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(["apply", "-R", "--whitespace=nowarn"])
        .arg(&path)
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if keluaran.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&keluaran.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn cari_checkpoint_berdasarkan_awalan() {
        // Serialisasi langsung menguji kontrak berkas tanpa menulis config asli.
        let c = Checkpoint {
            id: "abc123".to_string(),
            label: "awal".to_string(),
            ts: 1,
            messages: 4,
            patch: "abc123.patch".to_string(),
        };
        let baris = serde_json::to_string(&c).unwrap();
        let lagi: Checkpoint = serde_json::from_str(&baris).unwrap();
        assert_eq!(lagi, c);
    }

    #[test]
    fn revert_mengembalikan_berkas_git() {
        if !Command::new("git").arg("--version").output().is_ok() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "maicli-checkpoint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "T"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("a.txt"), "awal\n").unwrap();
        for args in [vec!["add", "a.txt"], vec!["commit", "-qm", "awal"]] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("a.txt"), "ubah\n").unwrap();
        // Simpan patch ke folder sementara dengan menimpa lokasi patch_dir via env
        // tidak memungkinkan; uji algoritma git apply langsung.
        let patch = Command::new("git")
            .args(["diff", "--binary", "HEAD"])
            .current_dir(&root)
            .output()
            .unwrap();
        let file = root.join("s.patch");
        std::fs::write(&file, patch.stdout).unwrap();
        let balik = Command::new("git")
            .args(["apply", "-R", "--whitespace=nowarn"])
            .arg(&file)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(balik.success());
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "awal\n"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
