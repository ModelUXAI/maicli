//! Skills (adopsi omp `docs/skills.md` versi v1): folder `skills/<nama>/SKILL.md`
//! ditemukan saat boot; daftarnya disuntikkan ke system prompt; tool `skill`
//! mengembalikan isi SKILL.md (tier Read — hanya baca, jalur dijaga).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::{Tool, ToolContext, ToolOutput, ToolTier};
use tui_wire::{Concurrency, ErrorCode, MxError, ToolDefinition};

/// Satu skill yang ditemukan.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    /// Nama (nama folder).
    pub nama: String,
    /// Deskripsi dari header `description:` (kosong bila tidak ada).
    pub deskripsi: String,
    /// Path SKILL.md.
    pub path: PathBuf,
}

/// Temukan skill: setiap subfolder langsung dari `dir` yang memuat `SKILL.md`.
pub fn temukan(dir: &Path) -> Vec<Skill> {
    let Ok(entri) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut hasil: Vec<Skill> = entri
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let path = e.path().join("SKILL.md");
            let nama = e.file_name().to_string_lossy().to_string();
            std::fs::metadata(&path).ok()?;
            Some(Skill {
                nama,
                deskripsi: deskripsi_dari(&path),
                path,
            })
        })
        .collect();
    hasil.sort_by(|a, b| a.nama.cmp(&b.nama));
    hasil
}

/// Baca deskripsi dari header: baris pertama yang diawali `description:`.
fn deskripsi_dari(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|teks| {
            teks.lines()
                .skip_while(|l| l.trim().is_empty())
                .find(|l| l.to_ascii_lowercase().starts_with("description:"))
                .map(|l| l[12..].trim().to_string())
        })
        .unwrap_or_default()
}

/// Daftar skill untuk disuntikkan ke system prompt.
pub fn ringkas_daftar(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut teks = String::from(
        "\n\n# Skills tersedia\n\nPanggil tool `skill` dengan parameter `name` untuk membaca isi skill:\n",
    );
    for skill in skills {
        teks.push_str(&format!(
            "- `{}` — {}\n",
            skill.nama,
            if skill.deskripsi.is_empty() {
                "(tanpa deskripsi)"
            } else {
                &skill.deskripsi
            }
        ));
    }
    teks
}

/// Tool `skill`: baca isi SKILL.md satu skill.
pub struct SkillTool {
    skills: Vec<Skill>,
}

impl SkillTool {
    /// Tool dari daftar skill hasil `temukan`.
    pub fn baru(skills: Vec<Skill>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "skill".to_string(),
            description: "Baca isi satu skill yang terdaftar di system prompt (parameter: name)"
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let nama = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| MxError::new(ErrorCode::Config, "argumen name wajib"))?;
        let Some(skill) = self.skills.iter().find(|s| s.nama == nama) else {
            let daftar: Vec<&str> = self.skills.iter().map(|s| s.nama.as_str()).collect();
            return Ok(ToolOutput::err(format!(
                "skill '{nama}' tidak ada; yang tersedia: {}",
                if daftar.is_empty() {
                    "(tidak ada)".to_string()
                } else {
                    daftar.join(", ")
                }
            )));
        };
        match std::fs::read_to_string(&skill.path) {
            Ok(isi) => Ok(ToolOutput::ok(isi)),
            Err(err) => Ok(ToolOutput::err(format!(
                "skill '{nama}' tidak bisa dibaca: {err}"
            ))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn siapkan(nama: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mx-skills-{}-{nama}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("uji-baca")).unwrap();
        std::fs::write(
            dir.join("uji-baca").join("SKILL.md"),
            "name: uji-baca\ndescription: Baca pola file uji\n\nIsi langkah:\n1. buka\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("kosong-folder")).unwrap(); // tanpa SKILL.md
        dir
    }

    #[test]
    fn temukan_mengabaikan_folder_tanpa_skill_md() {
        let dir = siapkan("temukan");
        let skills = temukan(&dir);
        assert_eq!(skills.len(), 1, "{skills:?}");
        assert_eq!(skills[0].nama, "uji-baca");
        assert_eq!(skills[0].deskripsi, "Baca pola file uji");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_tool_membaca_isi_dan_menolak_tidak_ada() {
        let dir = siapkan("tool");
        let skills = temukan(&dir);
        let tool = SkillTool::baru(skills);
        let ctx = ToolContext::new(&dir);
        let isi = tool
            .execute(&serde_json::json!({"name": "uji-baca"}), &ctx)
            .await
            .unwrap();
        assert!(!isi.is_error);
        assert!(isi.content.contains("Isi langkah"), "{}", isi.content);
        let salah = tool
            .execute(&serde_json::json!({"name": "tidak-ada"}), &ctx)
            .await
            .unwrap();
        assert!(salah.is_error);
        assert!(salah.content.contains("tidak ada"), "{}", salah.content);
        let tanpa_nama = tool
            .execute(&serde_json::json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(tanpa_nama.code, ErrorCode::Config);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ringkos_daftar_kosong_dan_terisi() {
        assert!(ringkas_daftar(&[]).is_empty());
        let teks = ringkas_daftar(&[Skill {
            nama: "uji".to_string(),
            deskripsi: "coba".to_string(),
            path: PathBuf::from("x"),
        }]);
        assert!(teks.contains("`uji`"), "{teks}");
        assert!(teks.contains("coba"), "{teks}");
    }
}
