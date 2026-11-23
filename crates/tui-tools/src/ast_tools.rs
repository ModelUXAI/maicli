//! `ast_grep` / `ast_edit`: pencarian & revisi struktural ringan.
//!
//! Ini bukan parser AST penuh: pola `$$NAMA` mencocokkan satu token identifier,
//! `$$$NAMA` mencocokkan rentang teks. Komentar dan string di-mask supaya pola
//! tidak cocok di dalam literal. `ast_edit` menghasilkan kartu proposal
//! (before/after) dan hanya menulis bila `apply: true` (tetap lewat approval
//! tier Write).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Satu kecocokan struktural.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Path relatif terhadap root sesi.
    pub path: String,
    /// Nomor baris 1-based.
    pub baris: usize,
    /// Teks baris yang cocok (dipangkas).
    pub teks: String,
}

/// Kompilasi pola menjadi regex + daftar nama variabel.
pub fn kompilasi(pola: &str) -> Result<(Regex, Vec<String>), String> {
    if pola.trim().is_empty() {
        return Err("pola tidak boleh kosong".to_string());
    }
    let mut out = String::from("(?s)");
    let mut nama = Vec::new();
    let mut sisa = pola;
    while let Some(pos) = sisa.find('$') {
        out.push_str(&regex::escape(&sisa[..pos]));
        sisa = &sisa[pos..];
        if sisa.starts_with("$$$") {
            let rest = &sisa[3..];
            let n = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .count();
            if n == 0 {
                return Err("$$$ butuh nama variabel".to_string());
            }
            let var = &rest[..n];
            nama.push(var.to_string());
            out.push_str(&format!("(?P<{var}>.+?)"));
            sisa = &rest[n..];
        } else if sisa.starts_with("$$") {
            let rest = &sisa[2..];
            let n = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .count();
            if n == 0 {
                return Err("$$ butuh nama variabel".to_string());
            }
            let var = &rest[..n];
            nama.push(var.to_string());
            out.push_str(&format!(r"(?P<{var}>[A-Za-z_][A-Za-z0-9_]*)"));
            sisa = &rest[n..];
        } else {
            out.push_str(r"\$");
            sisa = &sisa[1..];
        }
    }
    out.push_str(&regex::escape(sisa));
    Regex::new(&out)
        .map(|re| (re, nama))
        .map_err(|e| format!("pola tidak valid: {e}"))
}

/// Mask komentar dan string; panjang/baris tetap sama agar offset cocok.
pub fn mask_non_code(teks: &str) -> String {
    let bytes = teks.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0usize;
    let mut mode = 0u8; // 0=kode, 1=garis, 2=blok, 3=string
    while i < bytes.len() {
        match mode {
            0 => {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    mode = 1;
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    mode = 2;
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                } else if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
                    mode = 3;
                    out[i] = b' ';
                    i += 1;
                } else {
                    i += 1;
                }
            }
            1 => {
                if bytes[i] == b'\n' {
                    mode = 0;
                } else {
                    out[i] = b' ';
                }
                i += 1;
            }
            2 => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    mode = 0;
                    i += 2;
                } else {
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
            _ => {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                } else if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
                    out[i] = b' ';
                    mode = 0;
                    i += 1;
                } else {
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Terapkan template pengganti dengan variabel dari regex.
pub fn ganti(template: &str, caps: &regex::Captures) -> String {
    let mut out = String::with_capacity(template.len());
    let mut sisa = template;
    while let Some(pos) = sisa.find('$') {
        out.push_str(&sisa[..pos]);
        sisa = &sisa[pos + 1..];
        // Template memakai sintaks yang sama dengan pola: $$nama / $$$nama.
        if sisa.starts_with('$') {
            sisa = &sisa[1..];
        }
        let n = sisa
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if n == 0 {
            out.push('$');
            continue;
        }
        let var = &sisa[..n];
        out.push_str(caps.name(var).map(|m| m.as_str()).unwrap_or(var));
        sisa = &sisa[n..];
    }
    out.push_str(sisa);
    out
}

/// Kumpulkan berkas sumber yang dipindai (tanpa target/node_modules/.git).
fn berkas_sumber(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(root).into_iter().flatten() {
        let path = e.path();
        if !e.file_type().is_file() {
            continue;
        }
        if path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some(".git" | "target" | "node_modules")
            )
        }) {
            continue;
        }
        if path.extension().is_some_and(|ext| {
            matches!(
                ext.to_str().unwrap_or(""),
                "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp"
            )
        }) {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    out
}

fn daftar_path(root: &Path, arg: Option<&str>) -> Result<Vec<PathBuf>, String> {
    if let Some(relatif) = arg.filter(|s| !s.trim().is_empty()) {
        let path = root.join(relatif);
        if !path.is_file() {
            return Err(format!("bukan berkas: {relatif}"));
        }
        return Ok(vec![path]);
    }
    Ok(berkas_sumber(root))
}

fn relatif(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Cari pola pada satu kumpulan berkas.
pub fn cari(root: &Path, path: Option<&str>, pola: &str) -> Result<Vec<Match>, String> {
    let (re, _) = kompilasi(pola)?;
    let mut hasil = Vec::new();
    for file in daftar_path(root, path)? {
        let Ok(isi) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mask = mask_non_code(&isi);
        for (idx, baris) in mask.lines().enumerate() {
            if re.is_match(baris) {
                hasil.push(Match {
                    path: relatif(root, &file),
                    baris: idx + 1,
                    teks: isi
                        .lines()
                        .nth(idx)
                        .unwrap_or("")
                        .chars()
                        .take(240)
                        .collect(),
                });
            }
        }
    }
    Ok(hasil)
}

/// Hitung edit proposal untuk satu berkas; tidak menulis.
pub fn proposal(
    root: &Path,
    path: &str,
    pola: &str,
    pengganti: &str,
) -> Result<Vec<String>, String> {
    let (re, _) = kompilasi(pola)?;
    let file = root.join(path);
    let isi = std::fs::read_to_string(&file).map_err(|e| format!("gagal baca {path}: {e}"))?;
    let mask = mask_non_code(&isi);
    let mut hasil = Vec::new();
    for (idx, baris) in mask.lines().enumerate() {
        if let Some(caps) = re.captures(baris) {
            let asli = isi.lines().nth(idx).unwrap_or("");
            hasil.push(format!(
                "{}:{} - {}\n{}:{} + {}",
                path,
                idx + 1,
                asli,
                path,
                idx + 1,
                ganti(pengganti, &caps)
            ));
        }
    }
    Ok(hasil)
}

/// Hash pendek dari satu baris (anchor konteks untuk `hashline_edit`).
pub fn hash_baris(baris: &str) -> String {
    format!("{:x}", Sha256::digest(baris.as_bytes()))[..12].to_string()
}

/// Cari indeks baris berdasarkan anchor; hash opsional memecah duplikat.
pub fn cari_anchor(isi: &str, anchor: &str, hash: Option<&str>) -> Result<usize, String> {
    let cocok: Vec<usize> = isi
        .lines()
        .enumerate()
        .filter(|(_, baris)| baris.trim() == anchor.trim() || baris.contains(anchor))
        .filter(|(_, baris)| hash.is_none_or(|h| hash_baris(baris) == h))
        .map(|(i, _)| i)
        .collect();
    match cocok.as_slice() {
        [] => Err("anchor tidak ditemukan (atau hash tidak cocok)".to_string()),
        [satu] => Ok(*satu),
        _ => Err("anchor tidak unik; sertakan hash".to_string()),
    }
}

/// Edit baris dengan anchor + hash (proposal → apply).
pub struct HashlineEditTool;

#[async_trait]
impl Tool for HashlineEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "hashline_edit".to_string(),
            description: "Ganti satu baris memakai anchor/hash; apply=true menulis.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "anchor": { "type": "string" },
                    "hash": { "type": "string" },
                    "new_line": { "type": "string" },
                    "apply": { "type": "boolean" }
                },
                "required": ["path", "anchor", "new_line"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen path wajib"));
        };
        let Some(anchor) = args.get("anchor").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen anchor wajib"));
        };
        let Some(baru) = args.get("new_line").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen new_line wajib"));
        };
        let hash = args.get("hash").and_then(Value::as_str);
        let file = ctx.cwd.join(path);
        let isi = match std::fs::read_to_string(&file) {
            Ok(v) => v,
            Err(err) => return Ok(ToolOutput::err(format!("gagal baca {path}: {err}"))),
        };
        let indeks = match cari_anchor(&isi, anchor, hash) {
            Ok(v) => v,
            Err(err) => return Ok(ToolOutput::err(err)),
        };
        let lama = isi.lines().nth(indeks).unwrap_or("");
        let kartu = format!(
            "{}:{} - {}\n{}:{} + {}",
            path,
            indeks + 1,
            lama,
            path,
            indeks + 1,
            baru
        );
        if args.get("apply").and_then(Value::as_bool) != Some(true) {
            return Ok(ToolOutput::ok(format!(
                "PROPOSAL (belum ditulis):\n{kartu}"
            )));
        }
        let hasil: Vec<String> = isi
            .split_inclusive('\n')
            .enumerate()
            .map(|(i, baris)| {
                if i == indeks {
                    format!("{baru}\n")
                } else {
                    baris.to_string()
                }
            })
            .collect();
        match std::fs::write(&file, hasil.concat()) {
            Ok(()) => Ok(ToolOutput::ok(format!("{path}:{} diperbarui", indeks + 1))),
            Err(err) => Ok(ToolOutput::err(format!("gagal menulis {path}: {err}"))),
        }
    }
}

/// Tool pencarian struktural ringan.
pub struct AstGrepTool;

#[async_trait]
impl Tool for AstGrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ast_grep".to_string(),
            description:
                "Cari pola kode dengan wildcard $$nama / $$$nama (abaikan string & komentar)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string", "description": "Berkas tunggal; kosong = seluruh workspace" }
                },
                "required": ["pattern"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(pola) = args.get("pattern").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen pattern wajib"));
        };
        let path = args.get("path").and_then(Value::as_str);
        match cari(&ctx.cwd, path, pola) {
            Ok(m) if m.is_empty() => Ok(ToolOutput::ok("tidak ada kecocokan".to_string())),
            Ok(m) => {
                let baris: Vec<String> = m
                    .iter()
                    .take(200)
                    .map(|h| format!("{}:{}: {}", h.path, h.baris, h.teks))
                    .collect();
                Ok(ToolOutput::ok(baris.join("\n")))
            }
            Err(err) => Ok(ToolOutput::err(err)),
        }
    }
}

/// Tool revisi struktural dengan proposal → apply dua langkah.
pub struct AstEditTool;

#[async_trait]
impl Tool for AstEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ast_edit".to_string(),
            description: "Usulkan revisi pola kode; apply=true menulis setelah proposal disetujui."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "replacement": { "type": "string" },
                    "path": { "type": "string" },
                    "apply": { "type": "boolean", "description": "Default false = hanya proposal" }
                },
                "required": ["pattern", "replacement", "path"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(pola) = args.get("pattern").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen pattern wajib"));
        };
        let Some(pengganti) = args.get("replacement").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen replacement wajib"));
        };
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen path wajib"));
        };
        let kartu = match proposal(&ctx.cwd, path, pola, pengganti) {
            Ok(k) => k,
            Err(err) => return Ok(ToolOutput::err(err)),
        };
        if kartu.is_empty() {
            return Ok(ToolOutput::ok(
                "tidak ada kecocokan; tidak ada perubahan".to_string(),
            ));
        }
        if args.get("apply").and_then(Value::as_bool) != Some(true) {
            return Ok(ToolOutput::ok(format!(
                "PROPOSAL (belum ditulis; ulangi dengan apply=true bila disetujui):\n{}",
                kartu.join("\n")
            )));
        }
        let (re, _) = match kompilasi(pola) {
            Ok(v) => v,
            Err(err) => return Ok(ToolOutput::err(err)),
        };
        let file = ctx.cwd.join(path);
        let isi = match std::fs::read_to_string(&file) {
            Ok(v) => v,
            Err(err) => return Ok(ToolOutput::err(format!("gagal baca {path}: {err}"))),
        };
        let mut hasil = String::new();
        for baris in isi.split_inclusive('\n') {
            let tanpa = baris.trim_end_matches(['\r', '\n']);
            if let Some(caps) = re.captures(&mask_non_code(tanpa)) {
                hasil.push_str(&ganti(pengganti, &caps));
            } else {
                hasil.push_str(baris);
            }
            if !baris.ends_with('\n') {
                hasil.push('\n');
            }
        }
        match std::fs::write(&file, hasil) {
            Ok(()) => Ok(ToolOutput::ok(format!("{} terbitan diterapkan", path))),
            Err(err) => Ok(ToolOutput::err(format!("gagal menulis {path}: {err}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn root_test(nama: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maicli-ast-{nama}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wildcard_satu_token() {
        let (re, vars) = kompilasi("foo($$arg)").unwrap();
        assert_eq!(vars, vec!["arg".to_string()]);
        let caps = re.captures("foo(bar)").unwrap();
        assert_eq!(&caps["arg"], "bar");
        assert!(!re.is_match("foo(a.b)")); // bukan identifier tunggal
    }

    #[test]
    fn komentar_dan_string_diabaikan() {
        let src = "// foo(x)\nlet s = \"foo(y)\";\nfoo(z);\n";
        let mask = mask_non_code(src);
        assert!(!mask.contains("foo(x)"));
        assert!(!mask.contains("foo(y)"));
        assert!(mask.contains("foo(z)"));
    }

    #[tokio::test]
    async fn proposal_tidak_menulis_apply_menulis() {
        let root = root_test("apply");
        let path = root.join("a.rs");
        std::fs::write(&path, "foo(alpha);\n").unwrap();
        let ctx = ToolContext::new(&root);
        let args = json!({
            "pattern": "foo($$arg)",
            "replacement": "bar($$arg)",
            "path": "a.rs"
        });
        let lihat = AstEditTool
            .execute(&args, &ctx)
            .await
            .expect("proposal jalan");
        assert!(lihat.content.contains("PROPOSAL"), "{}", lihat.content);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("foo(alpha)")
        );

        let dengan_apply = json!({
            "pattern": "foo($$arg)",
            "replacement": "bar($$arg)",
            "path": "a.rs",
            "apply": true
        });
        AstEditTool
            .execute(&dengan_apply, &ctx)
            .await
            .expect("apply jalan");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("bar(alpha)")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn proposal_dan_penggantian_variabel() {
        let root = root_test("proposal");
        let path = root.join("a.rs");
        std::fs::write(&path, "foo(alpha);\n").unwrap();
        let kartu = proposal(&root, "a.rs", "foo($$arg)", "bar($$arg)").unwrap();
        assert_eq!(kartu.len(), 1);
        assert!(kartu[0].contains("bar(alpha)"), "{}", kartu[0]);
        std::fs::remove_dir_all(root).ok();
    }
    #[test]
    fn hashline_memilih_duplikat_dengan_hash() {
        let isi = "nilai = 1\nnilai = 2\n";
        let hash = hash_baris("nilai = 2");
        assert_eq!(cari_anchor(isi, "nilai =", Some(&hash)), Ok(1));
        assert!(cari_anchor(isi, "nilai =", None).is_err());
    }

    #[tokio::test]
    async fn hashline_apply_menulis() {
        let root = root_test("hashline");
        let path = root.join("a.rs");
        std::fs::write(&path, "nilai = 1\n").unwrap();
        let ctx = ToolContext::new(&root);
        let args = json!({
            "path": "a.rs",
            "anchor": "nilai =",
            "hash": hash_baris("nilai = 1"),
            "new_line": "nilai = 2",
            "apply": true
        });
        HashlineEditTool.execute(&args, &ctx).await.expect("apply");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("nilai = 2")
        );
        std::fs::remove_dir_all(root).ok();
    }
}
