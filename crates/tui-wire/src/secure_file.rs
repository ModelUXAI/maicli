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

    // `icacls /inheritance:r` tidak menghapus ACE eksplisit SYSTEM/Administrators
    // pada direktori temp runner CI: hasilnya 3 ACE dan `protected=False`, padahal
    // jaminan kita tepat 1 ACE untuk pengguna saat ini. Bangun DACL baru, aktifkan
    // perlindungan warisan, lalu isi dengan satu ACE FullControl untuk identitas
    // proses berjalan (`DOMAIN\user`) — bukan nama mentah yang bisa gagal resolve.
    let lokasi = path.display().to_string().replace('\'', "''");
    let skrip = format!(
        "$acl = New-Object System.Security.AccessControl.FileSecurity; \
         $acl.SetAccessRuleProtection($true, $false); \
         $id = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name; \
         $aturan = New-Object System.Security.AccessControl.FileSystemAccessRule($id, 'FullControl', 'Allow'); \
         $acl.AddAccessRule($aturan); \
         Set-Acl -LiteralPath '{lokasi}' -AclObject $acl"
    );
    // 0x0800_0000 = CREATE_NO_WINDOW: jangan memunculkan jendela konsol saat TUI.
    let keluaran = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &skrip])
        .creation_flags(0x0800_0000)
        .output()?;
    if keluaran.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Set-Acl gagal untuk {}: {}",
            path.display(),
            String::from_utf8_lossy(&keluaran.stderr).trim()
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

        if !path.exists() {
            return Ok(false);
        }
        // Kumpulkan semua identity ACE (bukan hanya yang pertama) + status proteksi.
        let skrip = format!(
            "$acl = Get-Acl -LiteralPath '{}'; \
             ($acl.Access | ForEach-Object {{ $_.IdentityReference.Value }}) -join '|'; \
             $acl.AreAccessRulesProtected",
            path.display().to_string().replace('\'', "''")
        );
        let keluaran = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &skrip])
            .creation_flags(0x0800_0000)
            .output()?;
        if !keluaran.status.success() {
            return Ok(false);
        }
        let teks = String::from_utf8_lossy(&keluaran.stdout);
        let mut baris = teks.lines().map(str::trim).filter(|s| !s.is_empty());
        let identitas = baris.next().unwrap_or_default().to_string();
        let terlindungi = baris.next() == Some("True");
        // Bandingkan dengan identitas proses (`DOMAIN\user`), bukan variabel
        // USERNAME saja — runner CI memakai `DOMAIN\USERNAME`.
        let id_proses = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[System.Security.Principal.WindowsIdentity]::GetCurrent().Name",
            ])
            .creation_flags(0x0800_0000)
            .output()?;
        if !id_proses.status.success() {
            return Ok(false);
        }
        let nama_proses = String::from_utf8_lossy(&id_proses.stdout)
            .trim()
            .to_ascii_lowercase();
        let hanya_pengguna = !identitas.is_empty()
            && identitas
                .split('|')
                .all(|ace| ace.trim().eq_ignore_ascii_case(&nama_proses));
        Ok(terlindungi && hanya_pengguna)
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
