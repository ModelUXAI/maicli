//! Grant approval yang bertahan antar-proses (`grants.json`).
//!
//! Hanya keputusan `Always` yang disimpan: `Once` dan `Session` sengaja tidak,
//! karena keduanya berlaku untuk satu panggilan atau satu sesi. File ditulis
//! atomik (tmp + rename) di direktori config pengguna, lalu ACL dipersempit ke
//! pengguna saat ini lewat `tui_wire::secure_file` (RENCANA §10 butir 9).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tui_wire::{ErrorCode, MxError, ToolTier};

use crate::Approver;
use crate::Decision;

/// Versi format file grant.
pub const GRANTS_VERSION: u32 = 1;

/// Satu grant yang disimpan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedGrant {
    /// Tier tool.
    pub tier: ToolTier,
    /// Subjek (path atau perintah).
    pub subject: String,
    /// Waktu kedaluwarsa (epoch ms); `None` = tidak kedaluwarsa (kompatibel file lama).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

/// Isi file grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantFile {
    /// Versi format.
    pub version: u32,
    /// Daftar grant.
    #[serde(default)]
    pub grants: Vec<PersistedGrant>,
}

impl Default for GrantFile {
    fn default() -> Self {
        Self {
            version: GRANTS_VERSION,
            grants: Vec::new(),
        }
    }
}

impl GrantFile {
    /// Muat file; file tidak ada berarti daftar kosong.
    ///
    /// File dengan versi lebih baru dari yang didukung ditolak agar skema yang
    /// belum dikenal tidak dibaca dengan asumsi lama (prinsip yang sama dengan
    /// skema sesi).
    pub fn load(path: &Path) -> Result<Self, MxError> {
        let file: Self = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|err| {
                MxError::new(
                    ErrorCode::Config,
                    format!("file grant {} tidak bisa diparse: {err}", path.display()),
                )
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(MxError::new(
                    ErrorCode::Config,
                    format!("file grant {} tidak bisa dibaca: {err}", path.display()),
                ));
            }
        };
        if file.version > GRANTS_VERSION {
            return Err(MxError::new(
                ErrorCode::Config,
                format!(
                    "file grant {} memakai versi {}, implementasi ini mendukung versi {}",
                    path.display(),
                    file.version,
                    GRANTS_VERSION
                ),
            ));
        }
        Ok(file)
    }

    /// Simpan atomik: tulis ke file sementara lalu rename.
    pub fn save(&self, path: &Path) -> Result<(), MxError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                MxError::new(
                    ErrorCode::Config,
                    format!("gagal membuat direktori {}: {err}", parent.display()),
                )
            })?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|err| {
            MxError::new(ErrorCode::Config, format!("gagal serialisasi grant: {err}"))
        })?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|err| {
            MxError::new(
                ErrorCode::Config,
                format!("gagal menulis {}: {err}", tmp.display()),
            )
        })?;
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path).map_err(|err| {
            MxError::new(
                ErrorCode::Config,
                format!("gagal memindahkan grant ke {}: {err}", path.display()),
            )
        })?;
        // Grant approval menentukan tool yang boleh berjalan: batasi ACL.
        tui_wire::secure_file::restrict_to_current_user(path).map_err(|err| {
            MxError::new(
                ErrorCode::Config,
                format!("gagal membatasi izin {}: {err}", path.display()),
            )
        })
    }

    /// Tambahkan grant tanpa kedaluwarsa; duplikat (tier+subjek) diabaikan.
    pub fn add(&mut self, tier: ToolTier, subject: impl Into<String>) {
        self.add_with_expiry(tier, subject, None);
    }

    /// Tambahkan grant dengan waktu kedaluwarsa opsional.
    ///
    /// Grant yang sudah ada untuk tier+subjek yang sama diperbarui masa berlakunya
    /// (keputusan `always` terbaru memperpanjang grant lama).
    pub fn add_with_expiry(
        &mut self,
        tier: ToolTier,
        subject: impl Into<String>,
        expires_at_ms: Option<u64>,
    ) {
        let subject = subject.into();
        if let Some(existing) = self
            .grants
            .iter_mut()
            .find(|grant| grant.tier == tier && grant.subject == subject)
        {
            existing.expires_at_ms = expires_at_ms;
            return;
        }
        self.grants.push(PersistedGrant {
            tier,
            subject,
            expires_at_ms,
        });
    }

    /// Hapus grant yang sudah kedaluwarsa pada waktu `now_ms`.
    pub fn retain_fresh(&mut self, now_ms: u64) {
        self.grants
            .retain(|grant| grant.expires_at_ms.is_none_or(|expires| expires > now_ms));
    }

    /// Apakah grant sudah ada.
    /// Hapus semua grant (dipakai `/forget-approvals`).
    pub fn clear(&mut self) {
        self.grants.clear();
    }

    pub fn contains(&self, tier: ToolTier, subject: &str) -> bool {
        self.contains_at(tier, subject, now_ms())
    }

    /// Seperti [`Self::contains`] tetapi waktu "sekarang" di-inject (test).
    pub fn contains_at(&self, tier: ToolTier, subject: &str, now_ms: u64) -> bool {
        self.grants.iter().any(|grant| {
            grant.tier == tier
                && grant.subject == subject
                && grant.expires_at_ms.is_none_or(|expires| expires > now_ms)
        })
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Approver yang menyimpan setiap keputusan `Always` ke file grant.
pub struct PersistingApprover {
    inner: Box<dyn Approver>,
    path: PathBuf,
    file: Mutex<GrantFile>,
    ttl: Option<std::time::Duration>,
}

impl PersistingApprover {
    /// Bungkus approver lain; isi file awal dipakai sebagai titik mulai.
    pub fn new(inner: Box<dyn Approver>, path: PathBuf, file: GrantFile) -> Self {
        Self::with_ttl(inner, path, file, None)
    }

    /// Seperti [`Self::new`], tetapi grant baru punya masa berlaku `ttl`
    /// (`None` = tidak kedaluwarsa, perilaku lama).
    pub fn with_ttl(
        inner: Box<dyn Approver>,
        path: PathBuf,
        file: GrantFile,
        ttl: Option<std::time::Duration>,
    ) -> Self {
        Self {
            inner,
            path,
            file: Mutex::new(file),
            ttl,
        }
    }

    /// Grant yang saat ini dipegang (untuk diagnostik/test).
    pub fn grants(&self) -> Vec<PersistedGrant> {
        self.file
            .lock()
            .map(|file| file.grants.clone())
            .unwrap_or_default()
    }

    async fn persist_always(&self, tier: ToolTier, subject: &str) {
        let snapshot = {
            let Ok(mut file) = self.file.lock() else {
                return;
            };
            file.add_with_expiry(
                tier,
                subject.to_string(),
                self.ttl
                    .map(|ttl| now_ms().saturating_add(ttl.as_millis() as u64)),
            );
            file.retain_fresh(now_ms());
            file.clone()
        };
        // Tulis di thread blocking: fs sinkron tidak boleh jalan di future.
        // Kegagalan tidak menjatuhkan turn; grant tetap berlaku untuk proses ini
        // lewat approver di dalam, dan file hanya untuk proses berikutnya.
        let path = self.path.clone();
        let _ = tokio::task::spawn_blocking(move || snapshot.save(&path)).await;
    }
}

impl PersistingApprover {
    /// Hapus semua grant dari memori dan dari file, lalu laporkan jumlahnya.
    pub fn forget_grants(&self) -> usize {
        match self.file.lock() {
            Ok(mut file) => {
                let count = file.grants.len();
                file.clear();
                let _ = file.save(&self.path);
                count
            }
            Err(_) => 0,
        }
    }

    /// Jumlah grant yang saat ini diingat.
    pub fn grant_count(&self) -> usize {
        self.file.lock().map(|file| file.grants.len()).unwrap_or(0)
    }
}

#[async_trait]
impl Approver for PersistingApprover {
    fn forget_grants(&self) {
        let _ = PersistingApprover::forget_grants(self);
    }

    async fn decide(&self, request: crate::ApprovalRequest) -> Decision {
        let tier = request.tier;
        let subject = request.subject.clone();
        let decision = self.inner.decide(request).await;
        if decision == Decision::Always {
            self.persist_always(tier, &subject).await;
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mx-grants-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("buat direktori");
        dir
    }

    #[test]
    fn grant_kedaluwarsa_diabaikan() {
        let mut file = GrantFile::default();
        file.add_with_expiry(ToolTier::Exec, "dir", Some(1_000));
        assert!(
            !file.contains_at(ToolTier::Exec, "dir", 1_001),
            "grant basi harus diabaikan"
        );
        assert!(
            file.contains_at(ToolTier::Exec, "dir", 999),
            "grant belum basi tetap berlaku"
        );
        file.retain_fresh(1_001);
        assert!(file.grants.is_empty(), "grant basi harus dipangkas");
    }

    #[test]
    fn grant_lama_tanpa_bidang_expiry_tetap_berlaku() {
        // File versi lama tidak punya expires_at_ms.
        let dir = temp_dir("kompat");
        let path = dir.join("grants.json");
        std::fs::write(
            &path,
            "{\"version\":1,\"grants\":[{\"tier\":\"exec\",\"subject\":\"dir\"}]}",
        )
        .expect("tulis");
        let file = GrantFile::load(&path).expect("load");
        assert!(file.contains_at(ToolTier::Exec, "dir", u64::MAX / 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn persisting_menyimpan_waktu_kedaluwarsa_saat_ttl_diisi() {
        let dir = temp_dir("ttl");
        let path = dir.join("grants.json");
        struct Always;
        #[async_trait]
        impl Approver for Always {
            async fn decide(&self, _request: crate::ApprovalRequest) -> Decision {
                Decision::Always
            }
        }
        let approver = PersistingApprover::with_ttl(
            Box::new(Always),
            path,
            GrantFile::default(),
            Some(std::time::Duration::from_secs(60)),
        );
        let request = crate::ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        };
        let outcome = approver.decide(request).await;
        assert_eq!(outcome, Decision::Always);
        let grants = approver.grants();
        assert_eq!(grants.len(), 1);
        let expires = grants[0]
            .expires_at_ms
            .expect("ttl harus mengisi expires_at_ms");
        assert!(expires > now_ms(), "waktu kedaluwarsa harus di masa depan");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_grant_versi_baru_ditolak() {
        let dir = temp_dir("versi-baru");
        let path = dir.join("grants.json");
        std::fs::write(&path, "{\"version\":99,\"grants\":[]}").unwrap();
        let err = GrantFile::load(&path).expect_err("versi baru harus ditolak");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("versi 99"), "{}", err.message);
        assert!(err.message.contains("mendukung versi 1"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_grant_dibatasi_ke_pengguna_saja() {
        let dir = temp_dir("acl");
        let path = dir.join("grants.json");
        let mut file = GrantFile::default();
        file.add(ToolTier::Read, "C:\\kerja\\a.txt");
        file.save(&path).expect("simpan");
        assert!(
            tui_wire::secure_file::acl_restricted(&path).unwrap(),
            "file grant harus 1 ACE dan terlindungi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_hilang_berarti_kosong() {
        let dir = temp_dir("hilang");
        let file = GrantFile::load(&dir.join("grants.json")).expect("load");
        assert_eq!(file, GrantFile::default());
        assert!(!file.contains(ToolTier::Exec, "dir"));
    }

    #[test]
    fn simpan_dan_muat_bolak_balik() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("grants.json");
        let mut file = GrantFile::default();
        file.add(ToolTier::Exec, "dir C:\\kerja");
        file.add(ToolTier::Write, "D:\\data");
        file.add(ToolTier::Exec, "dir C:\\kerja");
        assert_eq!(file.grants.len(), 2, "duplikat diabaikan");
        file.save(&path).expect("save");
        let loaded = GrantFile::load(&path).expect("load");
        assert_eq!(loaded, file);
        assert!(loaded.contains(ToolTier::Exec, "dir C:\\kerja"));
    }

    #[test]
    fn file_rusak_dilaporkan() {
        let dir = temp_dir("rusak");
        let path = dir.join("grants.json");
        std::fs::write(&path, b"bukan json").expect("tulis");
        let err = GrantFile::load(&path).expect_err("harus error");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(
            err.message.contains("tidak bisa diparse"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn approver_menyimpan_hanya_keputusan_always() {
        let dir = temp_dir("simpan");
        let path = dir.join("grants.json");
        let approver = PersistingApprover::new(
            Box::new(crate::FixedApprover::new(Decision::Always)),
            path.clone(),
            GrantFile::default(),
        );
        let request = crate::ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        };
        assert_eq!(approver.decide(request).await, Decision::Always);
        let loaded = GrantFile::load(&path).expect("load");
        assert!(loaded.contains(ToolTier::Exec, "dir"));

        let once_path = dir.join("once.json");
        let once = PersistingApprover::new(
            Box::new(crate::FixedApprover::new(Decision::Once)),
            once_path.clone(),
            GrantFile::default(),
        );
        let request = crate::ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        };
        assert_eq!(once.decide(request).await, Decision::Once);
        assert!(!once_path.exists(), "keputusan sekali tidak disimpan");
    }
    #[tokio::test]
    async fn forget_grants_mengosongkan_memori_dan_file() {
        let dir = temp_dir("forget");
        let path = dir.join("grants.json");
        let mut seed = GrantFile::default();
        seed.add(ToolTier::Exec, "dir");
        let approver = PersistingApprover::new(
            Box::new(crate::FixedApprover::new(Decision::Always)),
            path.clone(),
            seed,
        );
        assert_eq!(approver.grant_count(), 1);
        let removed = approver.forget_grants();
        assert_eq!(removed, 1);
        assert_eq!(approver.grant_count(), 0);
        let loaded = GrantFile::load(&path).expect("load");
        assert!(loaded.grants.is_empty(), "file ikut dikosongkan");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn kegagalan_menyimpan_tidak_menjatuhkan_keputusan() {
        let dir = temp_dir("simpan-gagal");
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"x").expect("tulis blocker");
        let path = blocker.join("grants.json");
        let approver = PersistingApprover::new(
            Box::new(crate::FixedApprover::new(Decision::Always)),
            path.clone(),
            GrantFile::default(),
        );
        let request = crate::ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        };
        assert_eq!(approver.decide(request).await, Decision::Always);
        assert!(
            approver
                .grants()
                .iter()
                .any(|grant| grant.tier == ToolTier::Exec && grant.subject == "dir"),
            "grant tetap berlaku di memori walau file gagal ditulis"
        );
        assert!(!path.exists(), "tidak ada file yang setengah jadi");
    }
}
