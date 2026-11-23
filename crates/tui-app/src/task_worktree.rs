//! Eksekusi task di worktree git terisolasi (`/task-worktree`).
//!
//! Anak adalah proses `maicli` terpisah dengan `--cwd` worktree; induk hanya
//! menerima ringkasan JSON ber-skema tetap. Worktree dihapus kecuali `keep`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// Path worktree untuk satu task.
pub fn worktree_path(id: &str) -> PathBuf {
    crate::config_root().join("worktrees").join(id)
}

/// Id unik berbasis waktu.
pub fn id_baru() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Buat worktree git terpisah dari HEAD.
pub fn buat(root: &Path, target: &Path) -> Result<(), String> {
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    let keluaran = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["worktree", "add", "--detach"])
        .arg(target)
        .arg("HEAD")
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if keluaran.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&keluaran.stderr).trim().to_string())
    }
}

/// Hapus worktree setelah task selesai.
pub fn hapus(root: &Path, target: &Path) -> Result<(), String> {
    let keluaran = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["worktree", "remove", "--force"])
        .arg(target)
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if keluaran.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&keluaran.stderr).trim().to_string())
    }
}

/// Ringkasan hasil dengan skema tetap.
pub fn hasil(id: &str, worktree: &str, sukses: bool, keluaran: &str) -> Value {
    let potong: String = keluaran.chars().take(4000).collect();
    json!({
        "status": if sukses { "ok" } else { "error" },
        "task_id": id,
        "worktree": worktree,
        "output": potong,
    })
}

/// Status keberhasilan dari kombinasi exit code dan teks anak.
pub fn sukses(exit_code: bool, keluaran: &str) -> bool {
    exit_code
        && !keluaran.contains("E_GATEWAY_AUTH")
        && !keluaran.contains("Error]")
        && !keluaran.contains("GAGAL")
}

/// Validasi skema hasil sebelum dikembalikan ke model/pengguna.
pub fn valid(nilai: &Value) -> bool {
    nilai
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s == "ok" || s == "error")
        && nilai
            .get("task_id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        && nilai
            .get("worktree")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        && nilai.get("output").and_then(Value::as_str).is_some()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn skema_hasil_valid() {
        let nilai = hasil("abc", "/tmp/wt", true, "selesai");
        assert!(valid(&nilai));
        assert!(!sukses(true, "E_GATEWAY_AUTH"));
        let mut rusak = nilai.clone();
        rusak.as_object_mut().unwrap().remove("output");
        assert!(!valid(&rusak));
    }

    #[test]
    fn buat_dan_hapus_worktree_git() {
        if !Command::new("git").arg("--version").output().is_ok() {
            return;
        }
        let dasar = std::env::temp_dir().join(format!(
            "maicli-worktree-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = dasar.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "T"],
        ] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repo.join("a.txt"), "awal\n").unwrap();
        for args in [vec!["add", "a.txt"], vec!["commit", "-qm", "awal"]] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let target = dasar.join("wt");
        buat(&repo, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt")).unwrap(),
            "awal\n"
        );
        hapus(&repo, &target).unwrap();
        assert!(!target.exists());
        std::fs::remove_dir_all(dasar).ok();
    }
}
