//! Resolusi konflik merge git (`conflict://N`).
//!
//! `conflict://N` memetakan ke daftar berkas konflik (`git diff --name-only
//! --diff-filter=U`). Memilih `ours/theirs/base` mengambil stage yang sesuai
//! (`:2:`/`:3:`/`:1:`) lalu `git add` berkas.

use std::path::Path;
use std::process::Command;

/// Parse `conflict://N` atau angka biasa menjadi indeks 0-based.
pub fn parse_uri(arg: &str) -> Option<usize> {
    let angka = arg.strip_prefix("conflict://").unwrap_or(arg);
    angka.parse::<usize>().ok()
}

/// Daftar berkas yang masih konflik.
pub fn daftar(cwd: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Ambil satu stage konflik dan tulis ke working tree, lalu `git add`.
pub fn pilih(cwd: &Path, path: &str, sisi: &str) -> Result<(), String> {
    let stage = match sisi {
        "ours" | "@ours" => "2",
        "theirs" | "@theirs" => "3",
        "base" | "@base" => "1",
        _ => return Err("sisi harus ours/theirs/base".to_string()),
    };
    let spec = format!(":{stage}:{path}");
    let isi = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .arg("show")
        .arg(&spec)
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if !isi.status.success() {
        return Err(format!(
            "stage {stage} tidak tersedia untuk {path}: {}",
            String::from_utf8_lossy(&isi.stderr).trim()
        ));
    }
    std::fs::write(cwd.join(path), &isi.stdout).map_err(|e| e.to_string())?;
    let add = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .arg("add")
        .arg(path)
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if add.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&add.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn parse_conflict_uri() {
        assert_eq!(parse_uri("conflict://0"), Some(0));
        assert_eq!(parse_uri("2"), Some(2));
        assert!(parse_uri("x").is_none());
    }

    #[test]
    fn daftar_dan_pilih_konflik() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "maicli-conflict-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        };
        assert!(git(&["init", "-q"]));
        assert!(git(&["config", "user.email", "t@example.com"]));
        assert!(git(&["config", "user.name", "T"]));
        assert!(git(&["config", "core.autocrlf", "false"]));
        std::fs::write(root.join("a.txt"), "base\n").unwrap();
        assert!(git(&["add", "a.txt"]));
        assert!(git(&["commit", "-qm", "base"]));
        assert!(git(&["checkout", "-qb", "fitur"]));
        std::fs::write(root.join("a.txt"), "fitur\n").unwrap();
        assert!(git(&["commit", "-qam", "fitur"]));
        assert!(git(&["checkout", "-q", "master"]));
        std::fs::write(root.join("a.txt"), "master\n").unwrap();
        assert!(git(&["commit", "-qam", "master"]));
        let _ = git(&["merge", "fitur"]);
        let konflik = daftar(&root).unwrap();
        assert_eq!(konflik, vec!["a.txt".to_string()]);
        pilih(&root, "a.txt", "ours").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "master\n"
        );
        assert!(daftar(&root).unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
    }
}
