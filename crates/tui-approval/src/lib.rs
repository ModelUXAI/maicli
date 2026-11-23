//! Gerbang approval: mode, urutan resolusi, grant, dan folder terlarang.
//!
//! Urutan resolusi mengikuti omp: folder terlarang mengalahkan segalanya,
//! lalu kebijakan per-tool, lalu keputusan bawaan tool, lalu tier vs mode
//! (dengan grant yang diingat). Apa pun yang bukan approval eksplisit = tolak.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
pub mod grants;
use serde::{Deserialize, Serialize};
use tui_wire::ToolTier;

/// Mode approval global.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    /// Semua tulis/exec bertanya; baca otomatis.
    #[default]
    AlwaysAsk,
    /// Seperti `AlwaysAsk`, tapi grant bisa diingat.
    AlwaysAskCached,
    /// Tulis otomatis; exec bertanya.
    Write,
    /// Semua otomatis kecuali folder terlarang.
    Yolo,
}

/// Kebijakan untuk satu tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Selalu boleh.
    Allow,
    /// Selalu tolak.
    Deny,
    /// Selalu tanya.
    Prompt,
}

/// Keputusan pengguna atas satu pemanggilan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Hanya pemanggilan ini.
    Once,
    /// Sepanjang sesi ini.
    Session,
    /// Selamanya (disimpan).
    Always,
    /// Tolak.
    Deny,
}

/// Permintaan approval untuk satu pemanggilan tool.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Nama tool.
    pub tool: String,
    /// Tier tool.
    pub tier: ToolTier,
    /// Subjek: perintah shell, path, atau ringkasan argumen.
    pub subject: String,
    /// Cuplikan yang ditampilkan ke pengguna.
    pub preview: String,
    /// Keputusan bawaan dari tool (`approval(args)`), bila ada.
    pub default: Option<Decision>,
}

/// Kunci grant: tier + subjek.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GrantKey {
    tier: ToolTier,
    subject: String,
}

/// Grant yang diingat.
#[derive(Debug, Default)]
pub struct GrantStore {
    session: HashSet<GrantKey>,
    always: HashSet<GrantKey>,
}

impl GrantStore {
    /// Apakah pemanggilan ini sudah punya grant.
    pub fn has(&self, tier: ToolTier, subject: &str) -> bool {
        let key = GrantKey {
            tier,
            subject: subject.to_string(),
        };
        self.session.contains(&key) || self.always.contains(&key)
    }

    /// Simpan grant sesuai keputusan.
    pub fn grant(&mut self, decision: Decision, tier: ToolTier, subject: &str) {
        let key = GrantKey {
            tier,
            subject: subject.to_string(),
        };
        match decision {
            Decision::Session => {
                self.session.insert(key);
            }
            Decision::Always => {
                self.session.insert(key.clone());
                self.always.insert(key);
            }
            Decision::Once | Decision::Deny => {}
        }
    }

    /// Masukkan grant persisten (dari file) ke kumpulan grant.
    pub fn seed_always(&mut self, tier: ToolTier, subject: impl Into<String>) {
        self.grant(Decision::Always, tier, &subject.into());
    }

    /// Jumlah grant yang diingat (untuk `/forget-approvals`).
    pub fn len(&self) -> usize {
        self.session.len() + self.always.len()
    }

    /// Apakah kosong.
    pub fn is_empty(&self) -> bool {
        self.session.is_empty() && self.always.is_empty()
    }

    /// Hapus semua grant.
    pub fn forget_all(&mut self) {
        self.session.clear();
        self.always.clear();
    }
}

/// Kebijakan approval: mode, kebijakan per-tool, dan folder terlarang.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApprovalPolicy {
    /// Mode global.
    pub mode: ApprovalMode,
    /// Kebijakan per nama tool.
    pub tools: BTreeMap<String, ToolPolicy>,
    /// Folder yang selalu meminta approval.
    #[serde(default)]
    pub restricted_folders: Vec<PathBuf>,
}

impl ApprovalPolicy {
    /// Mode `always-ask` dengan kebijakan kosong.
    pub fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Apakah subjek berada di folder terlarang.
    ///
    /// Subjek tool berkas berupa path sehingga dicek dengan `Path::starts_with`;
    /// subjek tool `exec` berupa **command string**, jadi folder terlarang juga
    /// dicocokkan sebagai substring (tanpa peduli huruf besar) supaya perintah
    /// yang menyebut path terlarang tetap meminta persetujuan (§10 butir 3).
    pub fn is_restricted(&self, subject: &str) -> bool {
        if self.restricted_folders.is_empty() {
            return false;
        }
        let candidate = Path::new(subject);
        let subjek_kecil = subject.to_lowercase();
        self.restricted_folders.iter().any(|folder| {
            candidate.starts_with(folder) || {
                let folder_kecil = folder.to_string_lossy().to_lowercase();
                !folder_kecil.is_empty() && subjek_kecil.contains(&folder_kecil)
            }
        })
    }

    /// Keputusan yang bisa diambil tanpa UI (`None` = harus bertanya).
    pub fn decide_without_ui(
        &self,
        request: &ApprovalRequest,
        grants: &GrantStore,
    ) -> Option<Decision> {
        // 1. Folder terlarang mengalahkan mode, grant, dan kebijakan tool.
        if self.is_restricted(&request.subject) {
            return None;
        }
        // 2. Kebijakan per-tool.
        match self.tools.get(&request.tool) {
            Some(ToolPolicy::Allow) => return Some(Decision::Once),
            Some(ToolPolicy::Deny) => return Some(Decision::Deny),
            Some(ToolPolicy::Prompt) => return None,
            None => {}
        }
        // 3. Keputusan bawaan tool (dihormati di semua mode).
        if let Some(default) = request.default {
            return Some(default);
        }
        // 4. Tier vs mode, dengan grant yang diingat.
        match self.mode {
            ApprovalMode::Yolo => Some(Decision::Once),
            ApprovalMode::AlwaysAsk => match request.tier {
                ToolTier::Read => Some(Decision::Once),
                ToolTier::Write | ToolTier::Exec => None,
            },
            ApprovalMode::AlwaysAskCached => match request.tier {
                ToolTier::Read => Some(Decision::Once),
                ToolTier::Write | ToolTier::Exec => {
                    if grants.has(request.tier, &request.subject) {
                        Some(Decision::Once)
                    } else {
                        None
                    }
                }
            },
            ApprovalMode::Write => match request.tier {
                ToolTier::Read | ToolTier::Write => Some(Decision::Once),
                ToolTier::Exec => {
                    if grants.has(request.tier, &request.subject) {
                        Some(Decision::Once)
                    } else {
                        None
                    }
                }
            },
        }
    }
}

/// Sumber keputusan untuk pemanggilan yang butuh manusia.
#[async_trait]
pub trait DecisionUi: Send + Sync {
    /// Tanyakan ke pengguna.
    async fn ask(&self, request: &ApprovalRequest) -> Decision;
}

/// Gerbang approval yang dipakai agent.
#[async_trait]
pub trait Approver: Send + Sync {
    /// Hapus semua grant yang diingat (dipakai `/forget-approvals`).
    fn forget_grants(&self) {}
    /// Putuskan satu pemanggilan tool.
    async fn decide(&self, request: ApprovalRequest) -> Decision;
}

/// Gerbang berbasis kebijakan, grant, dan UI opsional.
pub struct PolicyApprover {
    policy: ApprovalPolicy,
    grants: Mutex<GrantStore>,
    ui: Option<Box<dyn DecisionUi>>,
}

impl PolicyApprover {
    /// Masukkan grant persisten supaya tidak ditanyakan lagi.
    pub fn seed_always(&mut self, tier: ToolTier, subject: impl Into<String>) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.seed_always(tier, subject);
        }
    }

    /// Bangun gerbang tanpa UI: apa pun yang butuh manusia akan ditolak.
    pub fn new(policy: ApprovalPolicy) -> Self {
        Self {
            policy,
            grants: Mutex::new(GrantStore::default()),
            ui: None,
        }
    }

    /// Bangun gerbang dengan UI.
    pub fn with_ui(policy: ApprovalPolicy, ui: Box<dyn DecisionUi>) -> Self {
        Self {
            policy,
            grants: Mutex::new(GrantStore::default()),
            ui: Some(ui),
        }
    }

    /// Baca kebijakan efektif.
    pub fn policy(&self) -> &ApprovalPolicy {
        &self.policy
    }

    /// Jumlah grant yang diingat.
    pub fn grant_count(&self) -> usize {
        self.grants.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Lupakan semua grant.
    pub fn forget_grants(&self) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.forget_all();
        }
    }
}

#[async_trait]
impl Approver for PolicyApprover {
    fn forget_grants(&self) {
        PolicyApprover::forget_grants(self);
    }

    async fn decide(&self, request: ApprovalRequest) -> Decision {
        let pre = {
            let grants = self.grants.lock();
            match grants {
                Ok(grants) => self.policy.decide_without_ui(&request, &grants),
                Err(_) => None,
            }
        };
        if let Some(decision) = pre {
            return decision;
        }
        let Some(ui) = self.ui.as_ref() else {
            return Decision::Deny;
        };
        let decision = ui.ask(&request).await;
        if matches!(decision, Decision::Session | Decision::Always)
            && let Ok(mut grants) = self.grants.lock()
        {
            grants.grant(decision, request.tier, &request.subject);
        }
        decision
    }
}

/// Gerbang untuk test dan mode non-interaktif yang sudah disetujui.
pub struct FixedApprover {
    decision: Decision,
}

impl FixedApprover {
    /// Selalu memberi keputusan yang sama.
    pub fn new(decision: Decision) -> Self {
        Self { decision }
    }
}

#[async_trait]
impl Approver for FixedApprover {
    async fn decide(&self, _request: ApprovalRequest) -> Decision {
        self.decision
    }
}

#[cfg(test)]
mod tests {
    struct DenyUi;

    #[async_trait]
    impl DecisionUi for DenyUi {
        async fn ask(&self, _request: &ApprovalRequest) -> Decision {
            Decision::Deny
        }
    }

    fn exec_request() -> ApprovalRequest {
        ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        }
    }

    #[tokio::test]
    async fn mode_yolo_tidak_bertanya_ke_ui() {
        let approver =
            PolicyApprover::with_ui(ApprovalPolicy::new(ApprovalMode::Yolo), Box::new(DenyUi));
        assert_ne!(approver.decide(exec_request()).await, Decision::Deny);
    }

    #[tokio::test]
    async fn grant_persisten_dilewati_tanpa_bertanya() {
        let mut approver = PolicyApprover::with_ui(
            ApprovalPolicy::new(ApprovalMode::AlwaysAskCached),
            Box::new(DenyUi),
        );
        approver.seed_always(ToolTier::Exec, "dir");
        assert_ne!(
            approver.decide(exec_request()).await,
            Decision::Deny,
            "grant yang di-seed harus melewati UI"
        );
    }
    use super::*;

    fn request(tier: ToolTier, tool: &str, subject: &str) -> ApprovalRequest {
        ApprovalRequest {
            tool: tool.into(),
            tier,
            subject: subject.into(),
            preview: String::new(),
            default: None,
        }
    }

    #[test]
    fn matriks_mode_kali_tier() {
        let grants = GrantStore::default();
        let read = request(ToolTier::Read, "read", "a.txt");
        let write = request(ToolTier::Write, "write", "a.txt");
        let exec = request(ToolTier::Exec, "bash", "dir");

        let always_ask = ApprovalPolicy::new(ApprovalMode::AlwaysAsk);
        assert_eq!(
            always_ask.decide_without_ui(&read, &grants),
            Some(Decision::Once)
        );
        assert_eq!(always_ask.decide_without_ui(&write, &grants), None);
        assert_eq!(always_ask.decide_without_ui(&exec, &grants), None);

        let write_mode = ApprovalPolicy::new(ApprovalMode::Write);
        assert_eq!(
            write_mode.decide_without_ui(&read, &grants),
            Some(Decision::Once)
        );
        assert_eq!(
            write_mode.decide_without_ui(&write, &grants),
            Some(Decision::Once)
        );
        assert_eq!(write_mode.decide_without_ui(&exec, &grants), None);

        let yolo = ApprovalPolicy::new(ApprovalMode::Yolo);
        for req in [&read, &write, &exec] {
            assert_eq!(yolo.decide_without_ui(req, &grants), Some(Decision::Once));
        }
    }

    #[test]
    fn kebijakan_per_tool_menang_atas_mode() {
        let mut policy = ApprovalPolicy::new(ApprovalMode::Yolo);
        policy.tools.insert("bash".into(), ToolPolicy::Deny);
        policy.tools.insert("write".into(), ToolPolicy::Prompt);
        let grants = GrantStore::default();
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Exec, "bash", "dir"), &grants),
            Some(Decision::Deny)
        );
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Write, "write", "a"), &grants),
            None
        );
    }

    #[test]
    fn folder_terlarang_menutup_semua_tier_dan_mengalahkan_grant() {
        let mut policy = ApprovalPolicy::new(ApprovalMode::Yolo);
        policy.restricted_folders.push(PathBuf::from("C:\\rahasia"));
        let mut grants = GrantStore::default();
        // Grant `always` untuk path terlarang tetap tidak boleh auto-allow.
        grants.grant(Decision::Always, ToolTier::Read, "C:\\rahasia\\a.txt");
        for (tier, tool, subject) in [
            (ToolTier::Read, "read", "C:\\rahasia\\a.txt"),
            (ToolTier::Read, "grep", "C:\\rahasia"),
            (ToolTier::Write, "write", "C:\\rahasia\\b.txt"),
            (ToolTier::Write, "edit", "C:\\rahasia\\b.txt"),
            (ToolTier::Exec, "bash", "type C:\\rahasia\\a.txt"),
        ] {
            assert_eq!(
                policy.decide_without_ui(&request(tier, tool, subject), &grants),
                None,
                "{tool} di folder terlarang harus tetap bertanya"
            );
        }
        // Di luar folder tetap otomatis pada mode yolo.
        for (tier, tool, subject) in [
            (ToolTier::Read, "read", "C:\\aman\\a.txt"),
            (ToolTier::Write, "write", "C:\\aman\\b.txt"),
            (ToolTier::Exec, "bash", "dir"),
        ] {
            assert_eq!(
                policy.decide_without_ui(&request(tier, tool, subject), &grants),
                Some(Decision::Once),
                "{tool} di luar folder harus otomatis"
            );
        }
    }

    #[test]
    fn folder_terlarang_selalu_bertanya() {
        let mut policy = ApprovalPolicy::new(ApprovalMode::Yolo);
        policy.restricted_folders.push(PathBuf::from("C:\\rahasia"));
        let grants = GrantStore::default();
        assert_eq!(
            policy.decide_without_ui(
                &request(ToolTier::Read, "read", "C:\\rahasia\\a.txt"),
                &grants
            ),
            None
        );
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Read, "read", "C:\\aman\\a.txt"), &grants),
            Some(Decision::Once)
        );
    }

    #[test]
    fn grant_sesi_hanya_berlaku_untuk_subjek_sama() {
        let mut grants = GrantStore::default();
        grants.grant(Decision::Session, ToolTier::Exec, "dir");
        // Grant hanya dihormati oleh mode yang memang mengingat approval.
        let policy = ApprovalPolicy::new(ApprovalMode::AlwaysAskCached);
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Exec, "bash", "dir"), &grants),
            Some(Decision::Once)
        );
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Exec, "bash", "del"), &grants),
            None
        );
        assert_eq!(grants.len(), 1);
        grants.forget_all();
        assert!(grants.is_empty());
    }

    #[test]
    fn mode_always_ask_tidak_memakai_grant() {
        let mut grants = GrantStore::default();
        grants.grant(Decision::Always, ToolTier::Exec, "dir");
        let policy = ApprovalPolicy::new(ApprovalMode::AlwaysAsk);
        assert_eq!(
            policy.decide_without_ui(&request(ToolTier::Exec, "bash", "dir"), &grants),
            None
        );
    }

    #[test]
    fn tanpa_ui_permintaan_yang_butuh_manusia_ditolak() {
        let approver = PolicyApprover::new(ApprovalPolicy::new(ApprovalMode::AlwaysAsk));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let decision = runtime.block_on(approver.decide(request(ToolTier::Exec, "bash", "dir")));
        assert_eq!(decision, Decision::Deny);
    }

    #[test]
    fn default_tool_dihormati_di_semua_mode() {
        let mut policy = ApprovalPolicy::new(ApprovalMode::AlwaysAsk);
        let mut req = request(ToolTier::Exec, "bash", "dir");
        req.default = Some(Decision::Deny);
        assert_eq!(
            policy.decide_without_ui(&req, &GrantStore::default()),
            Some(Decision::Deny)
        );
        policy.mode = ApprovalMode::Yolo;
        assert_eq!(
            policy.decide_without_ui(&req, &GrantStore::default()),
            Some(Decision::Deny)
        );
    }
}
