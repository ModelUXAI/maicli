//! Mental model: berkas Markdown yang diringkas ke system prompt.

use std::path::{Path, PathBuf};

use tui_wire::{ErrorCode, MxError};

/// Satu mental model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentalModel {
    /// Nama model (nama berkas tanpa `.md`).
    pub name: String,
    /// Isi lengkap.
    pub isi: String,
    /// Ringkasan satu baris untuk prompt.
    pub ringkas: String,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::SessionCorrupt, pesan.into())
}

fn folder(root: &Path) -> PathBuf {
    root.join("mental-models")
}

fn nama_sah(nama: &str) -> bool {
    !nama.is_empty()
        && nama
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn path_model(root: &Path, name: &str) -> Result<PathBuf, MxError> {
    if !nama_sah(name) {
        return Err(galat(format!(
            "nama mental model '{name}' tidak sah (alfanumerik/-/_)"
        )));
    }
    Ok(folder(root).join(format!("{name}.md")))
}

fn ringkas(isi: &str) -> String {
    for line in isi.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let teks: String = line.chars().take(120).collect();
        return teks;
    }
    "(kosong)".to_string()
}

/// Daftar mental model, urut nama.
pub fn list(root: &Path) -> Result<Vec<MentalModel>, MxError> {
    let dir = folder(root);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(galat(format!("{}: {err}", dir.display()))),
    };
    let mut models = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !nama_sah(name) {
            continue;
        }
        let isi = std::fs::read_to_string(&path)
            .map_err(|e| galat(format!("{}: {e}", path.display())))?;
        models.push(MentalModel {
            name: name.to_string(),
            ringkas: ringkas(&isi),
            isi,
        });
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Baca satu mental model.
pub fn read(root: &Path, name: &str) -> Result<MentalModel, MxError> {
    let path = path_model(root, name)?;
    let isi = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            galat(format!("mental model '{name}' tidak ada"))
        } else {
            galat(format!("{}: {e}", path.display()))
        }
    })?;
    Ok(MentalModel {
        name: name.to_string(),
        ringkas: ringkas(&isi),
        isi,
    })
}

/// Tulis satu mental model (buat folder bila perlu).
pub fn write(root: &Path, name: &str, isi: &str) -> Result<MentalModel, MxError> {
    let path = path_model(root, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| galat(format!("{}: {e}", parent.display())))?;
    }
    let isi = isi.trim();
    if isi.is_empty() {
        return Err(galat("isi mental model tidak boleh kosong"));
    }
    std::fs::write(&path, format!("{isi}\n"))
        .map_err(|e| galat(format!("{}: {e}", path.display())))?;
    let _ = tui_wire::secure_file::restrict_to_current_user(&path);
    Ok(MentalModel {
        name: name.to_string(),
        ringkas: ringkas(isi),
        isi: isi.to_string(),
    })
}

/// Hapus satu mental model; `Ok(false)` bila tidak ada.
pub fn remove(root: &Path, name: &str) -> Result<bool, MxError> {
    let path = path_model(root, name)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(galat(format!("{}: {err}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn temp_dir(nama: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maicli-mm-{nama}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tulis_daftar_dan_baca() {
        let dir = temp_dir("tulis");
        let model = write(&dir, "arsitektur", "# Arsitektur\n\nGateway memakai Rust").unwrap();
        assert_eq!(model.ringkas, "Gateway memakai Rust");
        let daftar = list(&dir).unwrap();
        assert_eq!(daftar.len(), 1);
        assert_eq!(
            read(&dir, "arsitektur").unwrap().isi.trim_end(),
            "# Arsitektur\n\nGateway memakai Rust"
        );
    }

    #[test]
    fn nama_traversal_ditolak() {
        let dir = temp_dir("traversal");
        assert!(read(&dir, "../rahasia").is_err());
        assert!(write(&dir, "a/b", "x").is_err());
    }
}
