//! Implementasi `DecisionUi` untuk TUI (kanal ke UI) dan REPL teks (stdin).
//!
//! Kebijakan approval tetap dipegang `PolicyApprover`; berkas ini hanya
//! menyediakan jalur "tanya manusia" saat kebijakan tidak bisa memutuskan.

use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use tui_approval::grants::{GrantFile, PersistingApprover};
use tui_approval::{ApprovalPolicy, ApprovalRequest, Decision, DecisionUi, PolicyApprover};
use tui_wire::ToolTier;

/// Susun approver: kebijakan + UI + grant persisten dari `grants.json`.
pub fn build_approver(
    policy: ApprovalPolicy,
    ui: Box<dyn DecisionUi>,
    grants_path: std::path::PathBuf,
    grant_ttl: Option<std::time::Duration>,
) -> Result<PersistingApprover, tui_wire::MxError> {
    let mut file = GrantFile::load(&grants_path)?;
    // Grant basi tidak boleh di-seed; pemangkasan juga menyimpan hasil bersih.
    let sebelum = file.grants.len();
    file.retain_fresh(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0),
    );
    if file.grants.len() != sebelum {
        let _ = file.save(&grants_path);
    }
    let mut approver = PolicyApprover::with_ui(policy, ui);
    for grant in &file.grants {
        approver.seed_always(grant.tier, grant.subject.clone());
    }
    Ok(PersistingApprover::with_ttl(
        Box::new(approver),
        grants_path,
        file,
        grant_ttl,
    ))
}

/// Permintaan approval yang menunggu keputusan UI.
pub struct PendingApproval {
    /// Permintaan asli.
    pub request: ApprovalRequest,
    /// Kanal balasan; `Deny` bila UI menutup tanpa menjawab.
    pub reply: oneshot::Sender<Decision>,
}

impl PendingApproval {
    /// Detail verbatim untuk dialog inline: subjek dan argumen tanpa dipotong
    /// (mengikuti batas yang sudah diterapkan di sisi event/tool).
    pub fn verbatim_detail(&self) -> String {
        let mut text = format!("subjek: {}", self.request.subject);
        if !self.request.preview.trim().is_empty() {
            text.push_str("\nargumen:\n");
            text.push_str(self.request.preview.trim());
        }
        text
    }

    /// Ringkasan satu baris untuk ditampilkan.
    pub fn summary(&self) -> String {
        format!(
            "{} [{}] — {}",
            self.request.tool,
            tier_label(self.request.tier),
            shorten(&self.request.subject, 120)
        )
    }
}

pub(crate) fn tier_label(tier: ToolTier) -> &'static str {
    match tier {
        ToolTier::Read => "baca",
        ToolTier::Write => "tulis",
        ToolTier::Exec => "jalankan",
    }
}

pub(crate) fn shorten(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('\u{2026}');
    }
    out
}

/// UI approval untuk TUI: meneruskan permintaan lewat kanal.
pub struct ChannelDecisionUi {
    tx: UnboundedSender<PendingApproval>,
    timeout: Duration,
}

impl ChannelDecisionUi {
    /// Buat UI dengan kanal tujuan dan timeout keputusan.
    pub fn new(tx: UnboundedSender<PendingApproval>, timeout: Duration) -> Self {
        Self { tx, timeout }
    }
}

#[async_trait]
impl DecisionUi for ChannelDecisionUi {
    async fn ask(&self, request: &ApprovalRequest) -> Decision {
        let (reply, wait) = oneshot::channel();
        if self
            .tx
            .send(PendingApproval {
                request: request.clone(),
                reply,
            })
            .is_err()
        {
            return Decision::Deny;
        }
        match tokio::time::timeout(self.timeout, wait).await {
            Ok(Ok(decision)) => decision,
            // Timeout atau UI menutup tanpa jawaban: tolak, jangan menggantung.
            _ => Decision::Deny,
        }
    }
}

/// Teks permintaan approval untuk mode teks (stdout).
///
/// Menampilkan tool, tier, subjek, **argumen verbatim**, dan batas waktu supaya
/// persetujuan tidak diambil tanpa melihat apa yang akan dijalankan.
pub(crate) fn stdio_prompt(request: &ApprovalRequest, timeout: Duration) -> String {
    let mut text = format!(
        "\n[persetujuan] {} · {}\n  {}\n",
        request.tool,
        tier_label(request.tier),
        shorten(&request.subject, 200)
    );
    if !request.preview.trim().is_empty() && request.preview.trim() != request.subject.trim() {
        text.push_str("  argumen:\n");
        for line in request.preview.trim().lines() {
            text.push_str(&format!("    {line}\n"));
        }
    }
    text.push_str(&format!(
        "  batas waktu: {} detik\nIzinkan? [o] sekali / [s] sesi ini / [a] selalu / [t] tolak: ",
        timeout.as_secs()
    ));
    text
}

/// UI approval untuk mode teks: bertanya lewat stdin.
pub struct StdioDecisionUi {
    timeout: Duration,
}

impl StdioDecisionUi {
    /// Buat UI dengan timeout keputusan.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

#[async_trait]
impl DecisionUi for StdioDecisionUi {
    async fn ask(&self, request: &ApprovalRequest) -> Decision {
        let question = stdio_prompt(request, self.timeout);
        let answer = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            print!("{question}");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                return Decision::Deny;
            }
            match line.trim().chars().next() {
                Some('o' | 'O' | 'y' | 'Y') => Decision::Once,
                Some('s' | 'S') => Decision::Session,
                Some('a' | 'A') => Decision::Always,
                _ => Decision::Deny,
            }
        });
        match tokio::time::timeout(self.timeout, answer).await {
            Ok(inner) => inner.unwrap_or(Decision::Deny),
            Err(_) => Decision::Deny,
        }
    }
}

#[cfg(test)]
mod tests {
    // Test gagal keras bila asumsi tidak berlaku.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use tui_approval::Approver;

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir".to_string(),
            preview: "dir".to_string(),
            default: None,
        }
    }

    struct TolakSemua;

    #[async_trait]
    impl DecisionUi for TolakSemua {
        async fn ask(&self, _request: &ApprovalRequest) -> Decision {
            Decision::Deny
        }
    }

    fn grant_file(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mx-approval-ttl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir.join("grants.json")
    }

    #[tokio::test]
    async fn grant_kedaluwarsa_tidak_diseed_dan_basi_dipangkas() {
        let path = grant_file("basi");
        std::fs::write(
            &path,
            "{\"version\":1,\"grants\":[{\"tier\":\"exec\",\"subject\":\"dir\",\"expires_at_ms\":1}]}",
        )
        .expect("tulis grant basi");
        let approver = build_approver(
            ApprovalPolicy::from(tui_config::ApprovalConfig {
                mode: tui_approval::ApprovalMode::AlwaysAskCached,
                ..tui_config::ApprovalConfig::default()
            }),
            Box::new(TolakSemua),
            path.clone(),
            None,
        )
        .expect("approver");
        let outcome = approver.decide(request()).await;
        assert_eq!(outcome, Decision::Deny, "grant basi tidak boleh auto-allow");
        let teks = std::fs::read_to_string(&path).expect("file grant");
        assert!(
            !teks.contains("\"dir\""),
            "grant basi harus dipangkas: {teks}"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[tokio::test]
    async fn grant_belum_kedaluwarsa_tetap_dipakai() {
        let path = grant_file("segar");
        std::fs::write(
            &path,
            "{\"version\":1,\"grants\":[{\"tier\":\"exec\",\"subject\":\"dir\",\"expires_at_ms\":4102444800000}]}",
        )
        .expect("tulis grant segar");
        let approver = build_approver(
            ApprovalPolicy::from(tui_config::ApprovalConfig {
                mode: tui_approval::ApprovalMode::AlwaysAskCached,
                ..tui_config::ApprovalConfig::default()
            }),
            Box::new(TolakSemua),
            path.clone(),
            None,
        )
        .expect("approver");
        let outcome = approver.decide(request()).await;
        assert_eq!(
            outcome,
            Decision::Once,
            "grant segar harus otomatis diizinkan"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn prompt_mode_teks_memuat_argumen_verbatim_dan_batas_waktu() {
        let request = ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "Get-ChildItem".to_string(),
            preview: "Get-ChildItem -Recurse -Force | Sort-Object Length".to_string(),
            default: None,
        };
        let text = stdio_prompt(&request, Duration::from_secs(600));
        assert!(text.contains("bash"), "{text}");
        assert!(text.contains("bash · jalankan"), "tier tampil: {text}");
        assert!(text.contains("Get-ChildItem -Recurse -Force"), "{text}");
        assert!(text.contains("batas waktu: 600 detik"), "{text}");
        assert!(text.contains("[o] sekali"), "{text}");
        assert!(text.contains("[t] tolak"), "{text}");

        // Subjek sama dengan argumen: tidak diulang.
        let request = ApprovalRequest {
            tool: "read".to_string(),
            tier: ToolTier::Read,
            subject: "C:\\kerja\\a.txt".to_string(),
            preview: "C:\\kerja\\a.txt".to_string(),
            default: None,
        };
        let text = stdio_prompt(&request, Duration::from_secs(60));
        assert_eq!(text.matches("C:\\kerja\\a.txt").count(), 1, "{text}");
    }

    #[test]
    fn detail_verbatim_menampilkan_argumen_penuh() {
        let (tx, _rx) = oneshot::channel();
        let panjang = "x".repeat(400);
        let pending = PendingApproval {
            request: ApprovalRequest {
                tool: "bash".to_string(),
                tier: ToolTier::Exec,
                subject: "Get-ChildItem".to_string(),
                preview: panjang.clone(),
                default: None,
            },
            reply: tx,
        };
        let text = pending.verbatim_detail();
        assert!(text.contains("subjek: Get-ChildItem"), "{text}");
        assert!(text.contains(&panjang), "argumen 400 char harus utuh");
    }

    #[tokio::test]
    async fn channel_ui_menolak_setelah_timeout() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PendingApproval>();
        let ui = ChannelDecisionUi::new(tx, Duration::from_millis(20));
        assert_eq!(ui.ask(&request()).await, Decision::Deny);
        assert!(rx.try_recv().is_ok(), "permintaan tetap sampai ke UI");
    }

    #[tokio::test]
    async fn channel_ui_meneruskan_keputusan() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PendingApproval>();
        let ui = ChannelDecisionUi::new(tx, Duration::from_secs(5));
        tokio::spawn(async move {
            if let Some(pending) = rx.recv().await {
                let _ = pending.reply.send(Decision::Session);
            }
        });
        assert_eq!(ui.ask(&request()).await, Decision::Session);
    }
}
