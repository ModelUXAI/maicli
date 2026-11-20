//! Pembatasan izin file ke pengguna saat ini (RENCANA §10 butir 9).
//!
//! File config, grant approval, dan sesi berisi informasi sensitif. Di Windows
//! izin paling ketat yang didukung adalah ACL eksplisit untuk pengguna saja,
//! tanpa warisan dari folder induk. Implementasi memakai `icacls` bawaan
//! Windows supaya tidak perlu FFI `unsafe` (AGENTS §3) dan tetap bisa dibuktikan
//! dengan `acl_restricted`.
//!
//! Catatan: pada platform non-Windows fungsi ini tidak melakukan apa pun karena
//! produk ini Windows-native (RENCANA §1/V-06).

use std::io;
use std::path::Path;

/// Batasi izin file ke pengguna saat ini saja (matikan warisan ACL).
///
/// Gagal bila nama pengguna tidak diketahui atau `icacls` menolak; pemanggil
/// yang menulis file sensitif sebaiknya memperlakukan kegagalan ini sebagai
/// kegagalan keras agar file tidak tertinggal dengan izin longgar.
pub fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        restrict_windows(path)
    }
    #[cfg(all(unix, not(windows)))]
    {
        // POSIX: izin paling ketat yang didukung = 0600 (pemilik baca-tulis).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(windows)]
fn restrict_windows(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    let user = std::env::var("USERNAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| io::Error::other("USERNAME tidak tersedia untuk ACL"))?;
    // 0x0800_0000 = CREATE_NO_WINDOW: jangan memunculkan jendela konsol saat TUI.
    let output = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{user}:F"))
        .creation_flags(0x0800_0000)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "icacls gagal untuk {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stdout).trim()
        )))
    }
}

/// Apakah file hanya memberi akses kepada pengguna saat ini (diagnostik/test).
///
/// Membaca ACL nyata lewat PowerShell `Get-Acl`, bukan asumsi. Pada non-Windows
/// mengembalikan `false` karena jaminan ini khusus Windows.
pub fn acl_restricted(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let user = std::env::var("USERNAME").unwrap_or_default();
        if user.trim().is_empty() || !path.exists() {
            return Ok(false);
        }
        let script = format!(
            "$acl = Get-Acl -LiteralPath '{}'; \
             \"$($acl.Access.Count)|$($acl.Access[0].IdentityReference.Value)|$($acl.AreAccessRulesProtected)\"",
            path.display().to_string().replace('\'', "''")
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000)
            .output()?;
        if !output.status.success() {
            return Ok(false);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text.lines().next().unwrap_or_default().trim();
        let mut parts = line.split('|');
        let count = parts
            .next()
            .and_then(|value| value.trim().parse::<usize>().ok());
        let identity = parts.next().unwrap_or_default().to_string();
        let protected = parts.next().unwrap_or_default().trim() == "True";
        let names_current_user = identity
            .rsplit('\\')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(&user));
        Ok(count == Some(1) && protected && names_current_user)
    }
    #[cfg(not(windows))]
    {
        // POSIX: jaminan setara = mode pengguna-saja (600 / 0400): group+other
        // tidak punya izin apa pun. (Temuan verifikasi Linux 2026-11-20:
        // sebelumnya selalu false sehingga jaminan tidak pernah diuji di Linux.)
        use std::os::unix::fs::PermissionsExt;
        if !path.exists() {
            return Ok(false); // konsisten dengan Windows: file hilang = false
        }
        let meta = std::fs::metadata(path)?;
        let mode = meta.permissions().mode();
        Ok(mode & 0o077 == 0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("maicli-secure-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("file.jsonl");
        std::fs::write(&path, b"{}\n").expect("tulis");
        path
    }

    #[test]
    fn restrict_menghasilkan_acl_pengguna_saja() {
        let path = temp_file("acl");
        restrict_to_current_user(&path).expect("restrict");
        assert!(
            acl_restricted(&path).expect("periksa"),
            "ACL harus 1 ACE, terlindungi, pengguna saat ini"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn restrict_ulang_tetap_valid() {
        let path = temp_file("ulang");
        restrict_to_current_user(&path).expect("pertama");
        restrict_to_current_user(&path).expect("kedua");
        assert!(acl_restricted(&path).expect("periksa"));
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn file_tidak_ada_dilaporkan_error() {
        let missing = std::env::temp_dir().join("maicli-secure-tidak-ada");
        let _ = std::fs::remove_file(&missing);
        let result = restrict_to_current_user(&missing);
        assert!(result.is_err(), "path tidak ada harus error, bukan diam");
        assert!(!acl_restricted(&missing).expect("periksa"));
    }
    #[cfg(unix)]
    #[test]
    fn restrict_unix_menghasilkan_mode_600() {
        let dir = std::env::temp_dir().join(format!("maicli-sec-unix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("r.json");
        std::fs::write(&file, "{}").unwrap();
        restrict_to_current_user(&file).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode harus 600");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
