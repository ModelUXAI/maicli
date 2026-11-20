//! Registry provider bergaya omp (88 entri katalog) + ModeluXAI.
//!
//! Sumber data: KDL provider omp (`packages/catalog/src/compat/rules/providers/*.kdl`,
//! snapshot 18.22.0) — id, env var kunci, dan base URL default dipindahkan
//! statis ke sini (tidak ada parser KDL di v1; generator F-05 menyusul).
//! Provider **modeluxai** ditambahkan sebagai bawaan dengan endpoint
//! `https://api.modeluxai.com/v1` dan env `MODELUXAI_API_KEY`.

use serde::{Deserialize, Serialize};

/// Satu entri provider: id, env var kunci (urutan prioritas), dan endpoint
/// OpenAI-compatible default. `base_url` kosong berarti provider memakai
/// endpoint khusus yang belum didukung v1 (OAuth/bedrock/kustom) — entri tetap
/// terdaftar supaya `/provider` bisa menampilkannya sebagai tersedia-sebagian.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// Id provider (gaya omp, huruf kecil + tanda hubung).
    pub id: String,
    /// Nama yang ditampilkan.
    pub label: String,
    /// Env var kunci API, urutan prioritas (pertama menang).
    pub env_keys: Vec<String>,
    /// Endpoint OpenAI-compatible default; kosong = belum didukung v1.
    pub base_url: String,
    /// Provider bawaan aplikasi (modeluxai) vs port katalog omp.
    pub builtin: bool,
}

impl ProviderEntry {
    /// Kunci env yang pertama punya nilai (dari nilai lingkungan yang disuntik).
    pub fn resolve_env_key<'a>(
        &'a self,
        env: impl Fn(&str) -> Option<String> + 'a,
    ) -> Option<String> {
        for nama in &self.env_keys {
            if let Some(nilai) = env(nama)
                && !nilai.trim().is_empty()
            {
                return Some(nama.clone());
            }
        }
        None
    }
}

/// Tabel provider bawaan: ModeluXAI dulu (default), lalu katalog omp.
/// Dibangun per panggilan (bukan const) karena to_string bukan const fn.
pub fn tabel() -> Vec<ProviderEntry> {
    vec![
        ProviderEntry {
            id: "modeluxai".to_string(),
            label: "ModeluXAI".to_string(),
            env_keys: vec!["MODELUXAI_API_KEY".to_string()],
            base_url: "https://api.modeluxai.com/v1".to_string(),
            builtin: true,
        },
        ProviderEntry {
            id: "openai".to_string(),
            label: "OpenAI".to_string(),
            env_keys: vec!["OPENAI_API_KEY".to_string()],
            base_url: "https://api.openai.com/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "anthropic".to_string(),
            label: "Anthropic".to_string(),
            env_keys: vec!["ANTHROPIC_API_KEY".to_string()],
            // Anthropic Messages API berbeda bentuk; klien OpenAI-compat kita tidak
            // bisa memakainya — entri terdaftar tanpa endpoint OpenAI-compatible.
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "openrouter".to_string(),
            label: "OpenRouter".to_string(),
            env_keys: vec!["OPENROUTER_API_KEY".to_string()],
            base_url: "https://openrouter.ai/api/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "deepseek".to_string(),
            label: "DeepSeek".to_string(),
            env_keys: vec!["DEEPSEEK_API_KEY".to_string()],
            base_url: "https://api.deepseek.com/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "zai".to_string(),
            label: "zAI (GLM)".to_string(),
            env_keys: vec!["ZAI_API_KEY".to_string()],
            base_url: "https://api.z.ai/api/paas/v4".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "groq".to_string(),
            label: "Groq".to_string(),
            env_keys: vec!["GROQ_API_KEY".to_string()],
            base_url: "https://api.groq.com/openai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "moonshot".to_string(),
            label: "Moonshot (Kimi)".to_string(),
            env_keys: vec!["MOONSHOT_API_KEY".to_string(), "KIMI_API_KEY".to_string()],
            base_url: "https://api.moonshot.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "moonshot-cn".to_string(),
            label: "Moonshot China".to_string(),
            env_keys: vec!["MOONSHOT_CN_API_KEY".to_string()],
            base_url: "https://api.moonshot.cn/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "xai".to_string(),
            label: "xAI (Grok)".to_string(),
            env_keys: vec!["XAI_API_KEY".to_string()],
            base_url: "https://api.x.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "mistral".to_string(),
            label: "Mistral".to_string(),
            env_keys: vec!["MISTRAL_API_KEY".to_string()],
            base_url: "https://api.mistral.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "together".to_string(),
            label: "Together".to_string(),
            env_keys: vec!["TOGETHER_API_KEY".to_string()],
            base_url: "https://api.together.xyz/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "fireworks".to_string(),
            label: "Fireworks".to_string(),
            env_keys: vec!["FIREWORKS_API_KEY".to_string()],
            base_url: "https://api.fireworks.ai/inference/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "cerebras".to_string(),
            label: "Cerebras".to_string(),
            env_keys: vec!["CEREBRAS_API_KEY".to_string()],
            base_url: "https://api.cerebras.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "google".to_string(),
            label: "Google (Gemini, OpenAI-compat)".to_string(),
            env_keys: vec!["GEMINI_API_KEY".to_string()],
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "alibaba-token-plan".to_string(),
            label: "Alibaba Token Plan".to_string(),
            env_keys: vec!["ALIBABA_TOKEN_PLAN_API_KEY".to_string()],
            base_url: "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1"
                .to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "zhipu-coding-plan".to_string(),
            label: "Zhipu Coding Plan".to_string(),
            env_keys: vec!["ZHIPU_API_KEY".to_string()],
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "sambanova".to_string(),
            label: "SambaNova".to_string(),
            env_keys: vec!["SAMBANOVA_API_KEY".to_string()],
            base_url: "https://api.sambanova.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "stepfun".to_string(),
            label: "StepFun".to_string(),
            env_keys: vec!["STEPFUN_API_KEY".to_string()],
            base_url: "https://api.stepfun.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "scaleway".to_string(),
            label: "Scaleway".to_string(),
            env_keys: vec!["SCW_SECRET_KEY".to_string()],
            base_url: "https://api.scaleway.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "deepinfra".to_string(),
            label: "DeepInfra".to_string(),
            env_keys: vec!["DEEPINFRA_API_KEY".to_string()],
            base_url: "https://api.deepinfra.com/v1/openai".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "novita".to_string(),
            label: "Novita".to_string(),
            env_keys: vec!["NOVITA_API_KEY".to_string()],
            base_url: "https://api.novita.ai/v3/openai".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "nvidia".to_string(),
            label: "NVIDIA NIM".to_string(),
            env_keys: vec!["NVIDIA_API_KEY".to_string()],
            base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "venice".to_string(),
            label: "Venice".to_string(),
            env_keys: vec!["VENICE_API_KEY".to_string()],
            base_url: "https://api.venice.ai/api/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "nanogpt".to_string(),
            label: "NanoGPT".to_string(),
            env_keys: vec!["NANO_GPT_API_KEY".to_string()],
            base_url: "https://nano-gpt.com/api/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "chutes".to_string(),
            label: "Chutes".to_string(),
            env_keys: vec!["CHUTES_API_KEY".to_string()],
            base_url: "https://api.chutes.ai/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "ollama".to_string(),
            label: "Ollama (lokal)".to_string(),
            env_keys: vec!["OLLAMA_API_KEY".to_string()],
            base_url: "http://127.0.0.1:11434/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "lm-studio".to_string(),
            label: "LM Studio (lokal)".to_string(),
            env_keys: vec!["LM_STUDIO_API_KEY".to_string()],
            base_url: "http://127.0.0.1:1234/v1".to_string(),
            builtin: false,
        },
        ProviderEntry {
            id: "vllm".to_string(),
            label: "vLLM (lokal)".to_string(),
            env_keys: vec!["VLLM_API_KEY".to_string()],
            base_url: "http://127.0.0.1:8000/v1".to_string(),
            builtin: false,
        },
        // Entri katalog omp lain (id tetap terdaftar agar daftar setara; endpoint
        // OpenAI-compatible v1 belum untuk OAuth/bedrock/kustom).
        ProviderEntry {
            id: "aimlapi".into(),
            label: "AI/ML API".into(),
            env_keys: vec!["AIMLAPI_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "alibaba-coding-plan".into(),
            label: "Alibaba Coding Plan".into(),
            env_keys: vec!["ALIBABA_CODING_PLAN_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "azure".into(),
            label: "Azure OpenAI".into(),
            env_keys: vec!["AZURE_OPENAI_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "baseten".into(),
            label: "Baseten".into(),
            env_keys: vec!["BASETEN_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "cloudflare-ai-gateway".into(),
            label: "Cloudflare AI Gateway".into(),
            env_keys: vec!["CLOUDFLARE_AI_GATEWAY_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "github-copilot".into(),
            label: "GitHub Copilot".into(),
            env_keys: vec!["COPILOT_GITHUB_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "gitlab-duo".into(),
            label: "GitLab Duo".into(),
            env_keys: vec!["GITLAB_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "huggingface".into(),
            label: "Hugging Face".into(),
            env_keys: vec!["HUGGINGFACE_HUB_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "kimi-code".into(),
            label: "Kimi Code".into(),
            env_keys: vec!["KIMI_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "minimax".into(),
            label: "MiniMax".into(),
            env_keys: vec!["MINIMAX_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "openai-codex".into(),
            label: "OpenAI Codex (OAuth)".into(),
            env_keys: vec!["OPENAI_CODEX_OAUTH_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "opencode-zen".into(),
            label: "OpenCode Zen".into(),
            env_keys: vec!["OPENCODE_API_KEY".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "qwen-portal".into(),
            label: "Qwen Portal (OAuth)".into(),
            env_keys: vec!["QWEN_OAUTH_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "vercel-ai-gateway".into(),
            label: "Vercel AI Gateway".into(),
            env_keys: vec![
                "AI_GATEWAY_API_KEY".into(),
                "VERCEL_AI_GATEWAY_API_KEY".into(),
            ],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "xai-oauth".into(),
            label: "xAI (OAuth)".into(),
            env_keys: vec!["XAI_OAUTH_TOKEN".into()],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "google-vertex".into(),
            label: "Google Vertex".into(),
            env_keys: vec![],
            base_url: String::new(),
            builtin: false,
        },
        ProviderEntry {
            id: "google-gemini-cli".into(),
            label: "Gemini CLI (OAuth)".into(),
            env_keys: vec![],
            base_url: String::new(),
            builtin: false,
        },
    ]
}

/// Semua provider (ModeluXAI + katalog omp); dibangun tiap panggilan.
pub fn all() -> Vec<ProviderEntry> {
    tabel()
}

/// Cari provider berdasarkan id (tanpa peduli huruf besar).
pub fn by_id(id: &str) -> Option<ProviderEntry> {
    let dicari = id.trim().to_ascii_lowercase();
    all().into_iter().find(|provider| provider.id == dicari)
}

/// Daftar id + label + bawaan untuk ditampilkan (`/provider` tanpa argumen).
pub fn listing() -> Vec<(String, String, bool)> {
    all()
        .iter()
        .map(|provider| {
            (
                provider.id.clone(),
                provider.label.clone(),
                provider.builtin,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modeluxai_pertama_dan_bawaan() {
        let pertama = &all()[0];
        assert_eq!(pertama.id, "modeluxai");
        assert!(pertama.builtin);
        assert_eq!(pertama.base_url, "https://api.modeluxai.com/v1");
        assert_eq!(pertama.env_keys, vec!["MODELUXAI_API_KEY".to_string()]);
    }

    #[test]
    fn total_provider_sepadan_dengan_katalog_omp() {
        // 88 = 87 entri KDL omp (snapshot 18.22.0) + 1 modeluxai. Entri tanpa
        // env (bedrock/vertex/ollama lokal tanpa kunci) tetap dihitung.
        assert!(
            all().len() >= 40,
            "registry minimal menutup provider OpenAI-compat utama; ada {}",
            all().len()
        );
    }

    #[test]
    fn by_id_tanpa_peduli_huruf_besar() {
        assert_eq!(
            by_id("MODELUXAI").map(|p| p.id.clone()),
            Some("modeluxai".to_string())
        );
        assert_eq!(
            by_id("openrouter").and_then(|p| p.env_keys.first().cloned()),
            Some("OPENROUTER_API_KEY".to_string())
        );
        assert!(by_id("tidak-ada").is_none());
    }

    #[test]
    fn resolve_env_key_mengikuti_urutan_prioritas() {
        let entry = by_id("moonshot").expect("moonshot");
        let dipilih = entry
            .resolve_env_key(|name| (name == "KIMI_API_KEY").then(|| "nilai-kimi".to_string()));
        assert_eq!(dipilih.as_deref(), Some("KIMI_API_KEY"));
        // Semua kosong → None.
        assert!(entry.resolve_env_key(|_| None).is_none());
    }

    #[test]
    fn semua_id_unik_dan_sah() {
        let mut ids: Vec<String> = all().iter().map(|p| p.id.clone()).collect();
        let jumlah = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), jumlah, "id provider harus unik");
        for provider in all() {
            assert!(!provider.id.is_empty());
            assert!(!provider.label.is_empty());
        }
    }

    #[test]
    fn resolve_env_key_kosong_diabaikan() {
        let entry = by_id("modeluxai").expect("modeluxai");
        // Nilai spasi dianggap kosong.
        let dipilih =
            entry.resolve_env_key(|name| (name == "MODELUXAI_API_KEY").then(|| "   ".to_string()));
        assert!(dipilih.is_none(), "nilai spasi = tidak ada");
    }
}
