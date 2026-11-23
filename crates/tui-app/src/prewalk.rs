//! Prewalk: telusuri rencana sebelum eksekusi (read-only).
//!
//! Mengantrekan satu prompt yang meminta agen memetakan langkah, risiko, dan
//! berkas yang akan disentuh tanpa mengubah apa pun.

/// Prompt prewalk untuk goal + langkah awal.
pub fn prompt(goal: &str, langkah: &str) -> String {
    let tujuan = if goal.trim().is_empty() {
        "(tidak ada goal eksplisit)"
    } else {
        goal.trim()
    };
    let langkah = if langkah.trim().is_empty() {
        "(tentukan langkah pertama)"
    } else {
        langkah.trim()
    };
    format!(
        "PREWALK (read-only, jangan ubah berkas).\nTujuan: {tujuan}\nLangkah: {langkah}\n\n\
         Petakan: berkas yang akan disentuh, dependensi, risiko, dan urutan aman. \
         Berhenti setelah rencana; tunggu persetujuan sebelum eksekusi."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_memuat_goal_dan_readonly() {
        let teks = prompt("fitur X", "ubah modul");
        assert!(teks.contains("fitur X"));
        assert!(teks.contains("read-only"));
        assert!(prompt("", "").contains("tidak ada goal"));
    }
}
