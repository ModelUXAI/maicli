//! Impor instruksi proyek dari berkas konteks (`AGENTS.md`, `CLAUDE.md`,
//! `.cursorrules`, `.clinerules`, aturan Cursor `.mdc`, Copilot).
//!
//! Isi dibaca saat boot dan disuntikkan ke system prompt. Batas ukuran mencegah
//! satu berkas besar menghabiskan seluruh jendela konteks; kegagalan baca
//! diabaikan (instruksi proyek bersifat opsional).

use std::path::{Path, PathBuf};

/// Satu berkas konteks yang berhasil dibaca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFile {
    /// Path relatif terhadap root proyek.
    pub path: String,
    /// Sumber konvensi (codex, claude, cursor, cline, copilot).
    pub source: &'static str,
    /// Isi (mungkin terpotong).
    pub content: String,
    /// Apakah isi dipotong karena batas ukuran.
    pub truncated: bool,
}

/// Kap default per berkas dan total (byte UTF-8; dipotong di batas karakter).
pub const MAX_PER_FILE: usize = 64 * 1024;
pub const MAX_TOTAL: usize = 256 * 1024;

/// Kandidat berkas di root, sesuai prioritas tampil.
const ROOT_FILES: [(&str, &str); 5] = [
    ("AGENTS.md", "codex"),
    ("CLAUDE.md", "claude"),
    (".cursorrules", "cursor"),
    (".clinerules", "cline"),
    (".github/copilot-instructions.md", "copilot"),
];

/// Potong string di batas karakter UTF-8 (tanpa panic pada karakter multibyte).
fn potong(teks: &str, batas: usize) -> (String, bool) {
    if teks.len() <= batas {
        return (teks.to_string(), false);
    }
    let mut akhir = batas.min(teks.len());
    while akhir > 0 && !teks.is_char_boundary(akhir) {
        akhir -= 1;
    }
    (teks[..akhir].to_string(), true)
}

/// Muat semua berkas konteks yang ada di bawah `root`.
///
/// Path absolut di luar `root` tidak pernah dibaca. Urutan hasil deterministik:
/// berkas root sesuai `ROOT_FILES`, lalu aturan Cursor `.mdc` terurut.
pub fn muat(root: &Path) -> Vec<ContextFile> {
    let mut hasil = Vec::new();
    let mut total = 0usize;
    for (nama, sumber) in ROOT_FILES {
        let path = root.join(nama);
        if let Some(isi) = baca(&path) {
            let (isi, terpotong) = potong(&isi, MAX_PER_FILE);
            let sisa = MAX_TOTAL.saturating_sub(total);
            if sisa == 0 {
                break;
            }
            let (isi, terpotong2) = potong(&isi, sisa);
            total += isi.len();
            hasil.push(ContextFile {
                path: nama.to_string(),
                source: sumber,
                content: isi,
                truncated: terpotong || terpotong2,
            });
        }
    }
    let mut aturan: Vec<PathBuf> = Vec::new();
    let dir = root.join(".cursor").join("rules");
    if let Ok(entri) = std::fs::read_dir(dir) {
        for e in entri.flatten() {
            let path = e.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mdc"))
            {
                aturan.push(path);
            }
        }
    }
    aturan.sort();
    for path in aturan {
        let relatif = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let Some(isi) = baca(&path) else {
            continue;
        };
        let sisa = MAX_TOTAL.saturating_sub(total);
        if sisa == 0 {
            break;
        }
        let (isi, terpotong) = potong(&isi, MAX_PER_FILE.min(sisa));
        total += isi.len();
        hasil.push(ContextFile {
            path: relatif,
            source: "cursor",
            content: isi,
            truncated: terpotong,
        });
    }
    hasil
}

/// Baca satu berkas teks; `None` bila tidak ada/bukan UTF-8/gagal.
fn baca(path: &Path) -> Option<String> {
    String::from_utf8(std::fs::read(path).ok()?).ok()
}

/// Render blok system prompt; kosong bila tidak ada berkas.
pub fn blok(files: &[ContextFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Instruksi proyek (dari berkas konteks)\n");
    for file in files {
        out.push_str(&format!(
            "\n### `{}` ({})\n{}\n",
            file.path,
            file.source,
            if file.truncated {
                format!("{}\n[…dipotong pada batas ukuran]", file.content)
            } else {
                file.content.clone()
            }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn root_test(nama: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "maicli-context-{nama}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join(".cursor/rules")).unwrap();
        path
    }

    #[test]
    fn muat_berkas_root_dan_aturan_cursor() {
        let root = root_test("lengkap");
        std::fs::write(root.join("AGENTS.md"), "aturan codex").unwrap();
        std::fs::write(root.join(".cursorrules"), "aturan cursor").unwrap();
        std::fs::write(root.join(".cursor/rules/b.mdc"), "b aturan").unwrap();
        std::fs::write(root.join(".cursor/rules/a.mdc"), "a aturan").unwrap();
        let files = muat(&root);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].path, "AGENTS.md");
        assert_eq!(files[0].source, "codex");
        assert_eq!(files[2].path, ".cursor/rules/a.mdc");
        assert!(blok(&files).contains("a aturan"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn potong_di_batas_karakter_utf8() {
        let (isi, terpotong) = potong("ééé", 3);
        assert_eq!(isi, "é");
        assert!(terpotong);
    }

    #[test]
    fn direktori_kosong_menghasilkan_blok_kosong() {
        let root = root_test("kosong");
        assert!(muat(&root).is_empty());
        assert!(blok(&[]).is_empty());
        std::fs::remove_dir_all(root).ok();
    }
}
