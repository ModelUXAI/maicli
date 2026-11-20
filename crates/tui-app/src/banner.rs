//! Banner peringatan untuk mode approval `yolo` (RENCANA §10 butir 11).
//!
//! `yolo` mematikan gerbang persetujuan, jadi setiap mode yang bisa menjalankan
//! tool wajib menampilkannya sekali saat start. Banner tidak boleh dipakai untuk
//! mode lain dan `yolo` tidak pernah menjadi default tersimpan.

/// Teks banner yang wajib ditampilkan saat mode `yolo` aktif.
pub fn yolo_banner() -> &'static str {
    "PERINGATAN: mode yolo aktif — tool berjalan TANPA persetujuan. \
     Pakai hanya di lingkungan yang kamu percaya; ubah `approval.mode` untuk kembali aman."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_menyebut_yolo_dan_tanpa_persetujuan() {
        let text = yolo_banner();
        assert!(text.contains("yolo"), "{text}");
        assert!(text.to_lowercase().contains("tanpa persetujuan"), "{text}");
        assert!(text.contains("PERINGATAN"), "{text}");
        assert!(
            text.contains("approval.mode"),
            "banner harus menyebut cara mematikannya: {text}"
        );
    }
}
