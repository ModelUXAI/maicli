//! Marketplace extension: pasang/perbarui/hapus folder extension dari repo git.
//!
//! Keamanan: URL default wajib `https://`; `allow_insecure` hanya untuk repo
//! lokal saat pengembangan. Manifest divalidasi dengan parser ketat yang sama
//! dengan loader (`extension.json`), dan nama folder tidak boleh keluar dari
//! direktori extension.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tui_wire::{ErrorCode, MxError};

use crate::extensions::ExtensionManifest;

/// Extension yang terpasang di folder extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    /// Nama extension.
    pub name: String,
    /// Versi dari manifest.
    pub version: String,
    /// Folder extension.
    pub path: PathBuf,
}

/// Satu entry indeks marketplace.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    /// Nama extension.
    pub name: String,
    /// Versi.
    #[serde(default)]
    pub version: String,
    /// URL git.
    pub url: String,
    /// Deskripsi singkat.
    #[serde(default)]
    pub description: String,
}

/// Bentuk indeks marketplace (`index.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Index {
    /// Daftar extension.
    pub extensions: Vec<IndexEntry>,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::Config, pesan.into())
}

fn nama_sah(nama: &str) -> bool {
    !nama.is_empty()
        && nama
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Nama folder dari URL git (segmen terakhir tanpa `.git`).
pub fn nama_dari_url(url: &str) -> Result<String, MxError> {
    let tanpa_query = url.split(['?', '#']).next().unwrap_or(url);
    let segmen = tanpa_query
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    let nama = segmen.strip_suffix(".git").unwrap_or(segmen);
    if !nama_sah(nama) {
        return Err(galat(format!(
            "URL '{url}' tidak menghasilkan nama extension yang sah"
        )));
    }
    Ok(nama.to_string())
}

/// Validasi URL sumber sebelum menjalankan git.
pub fn valid_url(url: &str, allow_insecure: bool) -> Result<(), MxError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(galat("URL marketplace kosong"));
    }
    if url.starts_with("https://") {
        return Ok(());
    }
    if allow_insecure {
        if url.starts_with("http://") || url.starts_with("file://") || Path::new(url).is_dir() {
            return Ok(());
        }
        return Err(galat(
            "sumber harus https://, http://, file://, atau direktori lokal",
        ));
    }
    Err(galat(
        "sumber marketplace wajib https:// (set marketplace.allow_insecure untuk repo lokal)",
    ))
}

/// Validasi folder extension dan kembalikan manifest-nya.
pub fn inspect(folder: &Path) -> Result<InstalledExtension, MxError> {
    let nama_folder = folder
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| galat("folder extension tanpa nama"))?;
    if !nama_sah(nama_folder) {
        return Err(galat(format!("nama folder '{nama_folder}' tidak sah")));
    }
    let path = folder.join("extension.json");
    let isi =
        std::fs::read_to_string(&path).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    let manifest: ExtensionManifest =
        serde_json::from_str(&isi).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    if manifest.name != nama_folder {
        return Err(galat(format!(
            "manifest name '{}' ≠ folder '{nama_folder}'",
            manifest.name
        )));
    }
    Ok(InstalledExtension {
        name: manifest.name,
        version: manifest.version,
        path: folder.to_path_buf(),
    })
}

/// Daftar extension terpasang.
pub fn list(install_dir: &Path) -> Vec<InstalledExtension> {
    let Ok(entries) = std::fs::read_dir(install_dir) else {
        return Vec::new();
    };
    let mut hasil: Vec<InstalledExtension> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| inspect(&entry.path()).ok())
        .collect();
    hasil.sort_by(|a, b| a.name.cmp(&b.name));
    hasil
}

/// Hapus satu extension berdasarkan nama.
pub fn remove(name: &str, install_dir: &Path) -> Result<PathBuf, MxError> {
    if !nama_sah(name) {
        return Err(galat(format!("nama extension '{name}' tidak sah")));
    }
    let target = install_dir.join(name);
    if !target.is_dir() {
        return Err(galat(format!("extension '{name}' tidak terpasang")));
    }
    std::fs::remove_dir_all(&target).map_err(|e| galat(format!("{}: {e}", target.display())))?;
    Ok(target)
}

fn validasi_setelah_pasang(folder: &Path, nama: &str) -> Result<InstalledExtension, MxError> {
    let path = folder.join("extension.json");
    let isi =
        std::fs::read_to_string(&path).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    let manifest: ExtensionManifest =
        serde_json::from_str(&isi).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    if manifest.name != nama {
        return Err(galat(format!(
            "manifest name '{}' ≠ nama folder '{nama}'",
            manifest.name
        )));
    }
    for tool in &manifest.tools {
        if tool.name.is_empty()
            || !tool
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || tool.command.is_empty()
        {
            return Err(galat(format!(
                "tool '{}' di manifest tidak sah/tanpa command",
                tool.name
            )));
        }
    }
    Ok(InstalledExtension {
        name: manifest.name,
        version: manifest.version,
        path: folder.to_path_buf(),
    })
}

/// Pasang extension dari sumber (repo git lokal/remote): clone ke folder sementara,
/// validasi manifest, lalu pindahkan ke nama final.
pub async fn install_git(
    url: &str,
    install_dir: &Path,
    allow_insecure: bool,
) -> Result<InstalledExtension, MxError> {
    valid_url(url, allow_insecure)?;
    let nama = nama_dari_url(url)?;
    std::fs::create_dir_all(install_dir)
        .map_err(|e| galat(format!("{}: {e}", install_dir.display())))?;
    let target = install_dir.join(&nama);
    if target.exists() {
        return Err(galat(format!(
            "extension '{nama}' sudah terpasang; pakai update atau remove dulu"
        )));
    }
    let tmp = install_dir.join(format!(".{nama}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut command = tokio::process::Command::new("git");
    command
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--")
        .arg(url)
        .arg(&tmp)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .map_err(|_| galat("git clone melewati batas 120 detik"))?
        .map_err(|e| galat(format!("git tidak bisa dijalankan: {e}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(galat(format!("git clone gagal: {}", err.trim())));
    }
    let hasil = validasi_setelah_pasang(&tmp, &nama);
    if let Err(err) = hasil {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(err);
    }
    std::fs::rename(&tmp, &target).map_err(|e| galat(format!("{}: {e}", target.display())))?;
    inspect(&target)
}

/// Perbarui satu extension dengan `git pull --ff-only`.
pub async fn update(name: &str, install_dir: &Path) -> Result<InstalledExtension, MxError> {
    if !nama_sah(name) {
        return Err(galat(format!("nama extension '{name}' tidak sah")));
    }
    let folder = install_dir.join(name);
    if !folder.is_dir() {
        return Err(galat(format!("extension '{name}' tidak terpasang")));
    }
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(&folder)
        .args(["pull", "--ff-only"])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .map_err(|_| galat("git pull melewati batas 120 detik"))?
        .map_err(|e| galat(format!("git tidak bisa dijalankan: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(galat(format!("git pull gagal: {}", err.trim())));
    }
    inspect(&folder)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn dir(nama: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maicli-market-{nama}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn url_wajib_https_kecuali_diizinkan() {
        assert!(valid_url("https://contoh/repo.git", false).is_ok());
        assert!(valid_url("http://contoh/repo.git", false).is_err());
        assert!(valid_url("http://contoh/repo.git", true).is_ok());
        assert!(valid_url("https://contoh/repo.git", true).is_ok());
    }

    #[test]
    fn nama_dari_url_menolak_traversal() {
        assert_eq!(nama_dari_url("https://x/y/plugin.git").unwrap(), "plugin");
        assert!(nama_dari_url("https://x/..").is_err());
    }

    #[test]
    fn list_hanya_manifest_valid() {
        let root = dir("list");
        let baik = root.join("baik");
        std::fs::create_dir_all(&baik).unwrap();
        std::fs::write(
            baik.join("extension.json"),
            r#"{"name":"baik","version":"1.0","tools":[{"name":"echo","description":"x","command":["echo"]}]}"#,
        )
        .unwrap();
        let rusak = root.join("rusak");
        std::fs::create_dir_all(&rusak).unwrap();
        std::fs::write(rusak.join("extension.json"), "{").unwrap();
        let hasil = list(&root);
        assert_eq!(hasil.len(), 1);
        assert_eq!(hasil[0].name, "baik");
    }

    #[test]
    fn remove_menolak_nama_traversal() {
        let root = dir("remove");
        assert!(remove("../luar", &root).is_err());
        assert!(remove("tidak-ada", &root).is_err());
    }
}
