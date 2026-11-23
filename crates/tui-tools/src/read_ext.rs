//! Ringkasan berkas non-teks untuk tool `read`: SQLite, ZIP, TAR, PDF, biner.
//!
//! Ini bukan parser penuh: untuk arsip/PDF hanya daftar entry/metadata yang
//! diambil langsung dari header, cukup untuk memutuskan langkah berikutnya.

use std::path::Path;

/// Ambil ringkasan berdasarkan ekstensi; `None` = berkas teks biasa.
pub fn ringkas(path: &Path) -> Option<Result<String, String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "sqlite" | "db" | "sqlite3" => Some(sqlite(path)),
        "zip" => Some(zip(path)),
        "tar" => Some(tar(path)),
        "pdf" => Some(pdf(path)),
        _ => None,
    }
}

/// Ringkasan database SQLite: daftar tabel + jumlah baris (read-only).
pub fn sqlite(path: &Path) -> Result<String, String> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("gagal membuka SQLite: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(|e| e.to_string())?;
    let nama: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let mut baris = Vec::new();
    for nama in nama.iter().take(50) {
        // Nama berasal dari schema; tetap kuotasi untuk menghindari SQL injection.
        let aman = nama.replace('"', "\"\"");
        let hitung = format!("SELECT COUNT(*) FROM \"{aman}\"");
        let n: i64 = conn.query_row(&hitung, [], |r| r.get(0)).unwrap_or(0);
        baris.push(format!("- {nama}: {n} baris"));
    }
    Ok(format!(
        "SQLite: {} tabel\n{}",
        nama.len(),
        if baris.is_empty() {
            "(tidak ada tabel pengguna)".to_string()
        } else {
            baris.join("\n")
        }
    ))
}

/// Daftar entry ZIP dari local file header (stored/deflated sama).
pub fn zip(path: &Path) -> Result<String, String> {
    let isi = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 30 <= isi.len() && out.len() < 200 {
        if isi.get(pos..pos + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            pos += 1;
            continue;
        }
        let nama_len = u16::from_le_bytes([isi[pos + 26], isi[pos + 27]]) as usize;
        let size = u32::from_le_bytes([isi[pos + 22], isi[pos + 23], isi[pos + 24], isi[pos + 25]])
            as usize;
        if pos + 30 + nama_len > isi.len() {
            break;
        }
        let nama = String::from_utf8_lossy(&isi[pos + 30..pos + 30 + nama_len]).into_owned();
        out.push(format!("- {nama} ({size} byte)"));
        pos += 30 + nama_len + size;
    }
    Ok(format!("ZIP: {} entry\n{}", out.len(), out.join("\n")))
}

/// Daftar entry TAR dari header 512 byte.
pub fn tar(path: &Path) -> Result<String, String> {
    let isi = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 512 <= isi.len() && out.len() < 200 {
        let blok = &isi[pos..pos + 512];
        if blok.iter().all(|b| *b == 0) {
            break;
        }
        let nama = String::from_utf8_lossy(&blok[0..100])
            .trim_end_matches(char::from(0))
            .to_string();
        if nama.is_empty() {
            break;
        }
        let oktal = String::from_utf8_lossy(&blok[124..136]);
        let size = usize::from_str_radix(oktal.trim_matches(char::from(0)).trim(), 8).unwrap_or(0);
        out.push(format!("- {nama} ({size} byte)"));
        pos += 512 + size.div_ceil(512) * 512;
    }
    Ok(format!("TAR: {} entry\n{}", out.len(), out.join("\n")))
}

/// Ringkasan metadata PDF (tanpa dekompresi stream).
pub fn pdf(path: &Path) -> Result<String, String> {
    if let Ok(teks) = pdf_teks(path) {
        return Ok(teks);
    }
    let isi = std::fs::read(path).map_err(|e| e.to_string())?;
    if !isi.starts_with(b"%PDF-") {
        return Err("bukan berkas PDF (%PDF- tidak ditemukan)".to_string());
    }
    let teks = String::from_utf8_lossy(&isi);
    let halaman = teks
        .matches("/Type /Page")
        .count()
        .saturating_sub(teks.matches("/Type /Pages").count());
    Ok(format!(
        "PDF: {} byte ·versi {} · ~{} halaman (estimasi dari objek, teks belum diekstrak)",
        isi.len(),
        String::from_utf8_lossy(&isi[5..8.min(isi.len())]),
        halaman
    ))
}

/// Ambil teks dari satu operand operasi PDF (`Tj`/`TJ`).
fn operand_teks(obj: &lopdf::Object, out: &mut String) {
    match obj {
        lopdf::Object::String(bytes, _) => {
            if let Ok(teks) = obj.as_str() {
                out.push_str(&String::from_utf8_lossy(teks).replace("\\n", "\n"));
            } else {
                out.push_str(&String::from_utf8_lossy(bytes));
            }
        }
        lopdf::Object::Array(items) => {
            for item in items {
                operand_teks(item, out);
            }
        }
        _ => {}
    }
}

/// Ekstrak teks dari stream konten PDF yang sudah di-decode.
pub fn teks_dari_stream(data: &[u8]) -> String {
    let mut out = String::new();
    let Ok(konten) = lopdf::content::Content::decode(data) else {
        return out;
    };
    for op in &konten.operations {
        if matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
            for operand in &op.operands {
                operand_teks(operand, &mut out);
            }
            if matches!(op.operator.as_str(), "'" | "\"") {
                out.push('\n');
            }
        }
    }
    out
}

/// Ekstraksi teks per halaman dari berkas PDF.
pub fn pdf_teks(path: &Path) -> Result<String, String> {
    let doc = lopdf::Document::load(path).map_err(|e| format!("gagal membuka PDF: {e}"))?;
    let halaman = doc.get_pages();
    let mut hasil = Vec::new();
    for (nomor, id) in halaman {
        let mut teks = String::new();
        for content_id in doc.get_page_contents(id) {
            if let Ok(lopdf::Object::Stream(stream)) = doc.get_object(content_id)
                && let Ok(data) = stream.decompressed_content()
            {
                teks.push_str(&teks_dari_stream(&data));
            }
        }
        if !teks.trim().is_empty() {
            hasil.push(format!("--- halaman {nomor} ---\n{}", teks.trim()));
        }
    }
    if hasil.is_empty() {
        Err("PDF tidak memiliki teks yang bisa diekstrak".to_string())
    } else {
        Ok(hasil.join("\n\n"))
    }
}

/// Ringkasan biner: ukuran + 16 byte pertama.
pub fn biner(path: &Path) -> Result<String, String> {
    let isi = std::fs::read(path).map_err(|e| e.to_string())?;
    let awal: Vec<String> = isi.iter().take(16).map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "berkas biner: {} byte · header: {}",
        isi.len(),
        awal.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn root(nama: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "maicli-readext-{nama}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn sqlite_menampilkan_tabel() {
        let dir = root("sqlite");
        let path = dir.join("a.sqlite");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("CREATE TABLE catat (id INTEGER, teks TEXT)", [])
            .unwrap();
        conn.execute("INSERT INTO catat VALUES (1, 'hai')", [])
            .unwrap();
        drop(conn);
        let teks = sqlite(&path).unwrap();
        assert!(teks.contains("catat: 1 baris"), "{teks}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn zip_mendaftar_entry() {
        let dir = root("zip");
        let path = dir.join("a.zip");
        // Satu local header stored untuk "a.txt" dengan isi "hai".
        let mut isi = vec![0x50, 0x4b, 0x03, 0x04];
        isi.extend_from_slice(&20u16.to_le_bytes());
        isi.extend_from_slice(&[0u8; 12]);
        isi.extend_from_slice(&3u32.to_le_bytes());
        isi.extend_from_slice(&3u32.to_le_bytes());
        isi.extend_from_slice(&5u16.to_le_bytes());
        isi.extend_from_slice(&0u16.to_le_bytes());
        isi.extend_from_slice(b"a.txt");
        isi.extend_from_slice(b"hai");
        std::fs::write(&path, isi).unwrap();
        let teks = zip(&path).unwrap();
        assert!(teks.contains("a.txt (3 byte)"), "{teks}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn biner_menampilkan_header() {
        let dir = root("biner");
        let path = dir.join("a.bin");
        std::fs::write(&path, [0x7f, 0x45, 0x4c, 0x46]).unwrap();
        assert!(biner(&path).unwrap().contains("7f 45 4c 46"));
        std::fs::remove_dir_all(dir).ok();
    }
    #[test]
    fn teks_dari_stream_pdf() {
        let stream = b"BT /F1 12 Tf 72 720 Td (Halo PDF) Tj ET\n";
        assert!(teks_dari_stream(stream).contains("Halo PDF"));
    }
}
