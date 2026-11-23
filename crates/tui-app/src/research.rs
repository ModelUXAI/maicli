//! Autoresearch terarah: satu pertanyaan → beberapa putaran prompt read-only.
//!
//! Tidak ada browsing otomatis di sini; tiap putaran meminta agen memakai tool
//! read-only yang tersedia, lalu menyimpulkan temuan. Putaran dibatasi 2..5.

/// Susun prompt untuk satu putaran riset.
pub fn prompt_putaran(pertanyaan: &str, putaran: u32) -> String {
    format!(
        "Riset (putaran {putaran}): {pertanyaan}\n\
         Gunakan hanya tool read-only. Kumpulkan bukti dari berkas/dokumen yang ada, \
         catat path/nama sumber, dan akhiri dengan temuan singkat + pertanyaan terbuka. \
         Jangan mengubah berkas."
    )
}

/// Parse jumlah putaran (default 3, batas 2..5).
pub fn putaran(arg: &str) -> Result<u32, String> {
    if arg.trim().is_empty() {
        return Ok(3);
    }
    let n: u32 = arg
        .trim()
        .parse()
        .map_err(|_| "jumlah putaran harus angka".to_string())?;
    if (2..=5).contains(&n) {
        Ok(n)
    } else {
        Err("jumlah putaran harus 2..5".to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn prompt_dan_batas_putaran() {
        assert!(prompt_putaran("topik", 2).contains("read-only"));
        assert_eq!(putaran("").unwrap(), 3);
        assert_eq!(putaran("5").unwrap(), 5);
        assert!(putaran("1").is_err());
        assert!(putaran("6").is_err());
    }
}
