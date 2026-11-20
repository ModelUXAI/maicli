//! Registry slash command v1.
//!
//! Command tidak pernah memanggil model; ia mengembalikan hasil yang dipakai
//! UI (teks balasan, ganti model, keluar). Menambah command = menambah baris di
//! `COMMANDS` + satu cabang di `dispatch` + test.

use std::path::Path;

/// Konteks yang dibutuhkan command.
pub struct CommandContext<'a> {
    /// Model aktif sesi.
    pub model: &'a str,
    /// Provider aktif (id registry) dan env var kuncinya.
    pub provider: (&'a str, &'a str),
    /// Nama tool yang terdaftar.
    pub tools: &'a [String],
    /// Path file sesi, bila sudah dibuat.
    pub session_path: Option<&'a Path>,
    /// Id model yang diketahui dari discovery gateway (boleh kosong).
    pub known_models: &'a [String],
    /// Direktori sesi (untuk `/tree`); None = tidak tersedia.
    pub sessions_dir: Option<&'a Path>,
    /// Ringkasan skill aktif (untuk `/skills`); kosong = tidak ada skill.
    pub skills_ringkas: &'a str,
    /// Window konteks dari config (untuk `/extended-context`).
    pub context_window: Option<u64>,
    /// Tingkat penalaran aktif (untuk `/effort` tanpa argumen).
    pub effort: Option<&'a str>,
}

/// Hasil eksekusi satu baris input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// Tampilkan teks ini ke pengguna.
    Reply(String),
    /// Ganti model sesi ke id ini.
    SetModel(String),
    /// Ringkas konteks sekarang.
    Compact,
    /// Jeda sesi.
    Pause,
    /// Lanjutkan sesi yang dijeda.
    Resume,
    /// Tampilkan pemakaian token.
    Usage,
    /// Tampilkan daftar sesi tersimpan (opsional difilter judul).
    Sessions(Option<String>),
    /// Pindah ke sesi lain (`None` = `latest`).
    SwitchSession(Option<String>),
    /// Ganti judul sesi.
    SetTitle(String),
    /// Tampilkan pemakaian konteks terhadap anggaran.
    Context,
    /// Tampilkan daftar todo sesi.
    Todo,
    /// Mulai sesi baru (mengosongkan konteks).
    NewSession,
    /// Duplikasi sesi sekarang lalu lanjutkan salinannya.
    ForkSession,
    /// Tampilkan pohon sesi (fork; kedalaman = cabang).
    Tree,
    /// Hapus semua grant approval yang diingat.
    ForgetApprovals,
    /// Bersihkan layar timeline (UI).
    ClearScreen,
    /// Set tingkat penalaran (`low`/`medium`/`high`); `None` = default gateway.
    SetEffort(Option<String>),
    /// Set window konteks token (Some = extended; None = kembali config).
    SetContextWindow(Option<u64>),
    /// Aktifkan yolo hanya untuk turn berikutnya.
    ForceOnce,
    /// Tampilkan daftar provider (tanpa argumen) atau ganti provider aktif.
    SetProvider(Option<String>),
    /// Keluar dari sesi.
    Quit,
    /// Perintah tidak dikenal.
    Unknown(String),
    /// Bukan command (kirim ke model).
    NotACommand,
}

/// Daftar command v1 beserta deskripsi satu baris.
/// Satu command builtin beserta atributnya (C-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Nama tanpa garis miring depan.
    pub name: &'static str,
    /// Pemakaian argumen, mis. `<>` atau `[id]`; kosong berarti tanpa argumen.
    pub usage: &'static str,
    /// Nama alternatif yang setara.
    pub aliases: &'static [&'static str],
    /// Deskripsi satu baris untuk `/help`.
    pub description: &'static str,
    /// Apakah command menerima argumen.
    pub allow_args: bool,
}

/// Registry command builtin v1 (satu-satunya precedence di v1).
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "help",
        usage: "",
        aliases: &["?"],
        description: "daftar perintah",
        allow_args: false,
    },
    CommandSpec {
        name: "model",
        usage: "[id]",
        aliases: &[],
        description: "tampilkan atau ganti model sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "compact",
        usage: "",
        aliases: &[],
        description: "ringkas konteks sekarang",
        allow_args: false,
    },
    CommandSpec {
        name: "pause",
        usage: "",
        aliases: &[],
        description: "jeda sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "resume",
        usage: "",
        aliases: &[],
        description: "lanjutkan sesi yang dijeda",
        allow_args: false,
    },
    CommandSpec {
        name: "usage",
        usage: "",
        aliases: &[],
        description: "pemakaian token sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "sessions",
        usage: "[judul]",
        aliases: &[],
        description: "daftar sesi tersimpan",
        allow_args: true,
    },
    CommandSpec {
        name: "tree",
        usage: "",
        aliases: &[],
        description: "pohon sesi (fork)",
        allow_args: false,
    },
    CommandSpec {
        name: "switch",
        usage: "[id]",
        aliases: &[],
        description: "pindah sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "title",
        usage: "<teks>",
        aliases: &[],
        description: "ganti judul sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "context",
        usage: "",
        aliases: &[],
        description: "pemakaian konteks vs anggaran",
        allow_args: false,
    },
    CommandSpec {
        name: "todo",
        usage: "",
        aliases: &[],
        description: "daftar todo sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "new",
        usage: "",
        aliases: &[],
        description: "mulai sesi baru",
        allow_args: false,
    },
    CommandSpec {
        name: "fork",
        usage: "",
        aliases: &[],
        description: "duplikasi sesi lalu lanjutkan salinannya",
        allow_args: false,
    },
    CommandSpec {
        name: "tools",
        usage: "",
        aliases: &[],
        description: "daftar tool aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "session",
        usage: "",
        aliases: &[],
        description: "path file sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "provider",
        usage: "[id]",
        aliases: &[],
        description: "daftar atau ganti provider gateway",
        allow_args: true,
    },
    CommandSpec {
        name: "clear",
        usage: "",
        aliases: &["cls"],
        description: "bersihkan layar timeline",
        allow_args: false,
    },
    CommandSpec {
        name: "skills",
        usage: "",
        aliases: &[],
        description: "daftar skill aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "effort",
        usage: "[low|medium|high]",
        aliases: &[],
        description: "tingkat penalaran model (tanpa argumen = lihat sekarang)",
        allow_args: true,
    },
    CommandSpec {
        name: "extended-context",
        usage: "[on|off]",
        aliases: &["extctx"],
        description: "pakai window konteks besar (on) atau kembali ke config (off)",
        allow_args: true,
    },
    CommandSpec {
        name: "force",
        usage: "",
        aliases: &["yolo"],
        description: "turn berikutnya berjalan tanpa approval (sekali)",
        allow_args: false,
    },
    CommandSpec {
        name: "forget-approvals",
        usage: "",
        aliases: &[],
        description: "hapus semua grant approval",
        allow_args: false,
    },
    CommandSpec {
        name: "quit",
        usage: "",
        aliases: &["exit"],
        description: "keluar dari sesi",
        allow_args: false,
    },
];

impl CommandSpec {
    /// Teks pemakaian untuk `/help`, mis. `/title <teks>`.
    pub fn display(&self) -> String {
        if self.usage.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{} {}", self.name, self.usage)
        }
    }
}

/// Cari command berdasarkan nama atau alias (`help` / `?`).
pub fn find_command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// Daftar command dalam bentuk teks untuk `/help`.
pub fn help_text() -> String {
    let mut out = String::from("perintah yang tersedia:\n");
    for spec in COMMANDS {
        out.push_str(&format!("  {:<14} {}\n", spec.display(), spec.description));
    }
    out.push_str("selain itu, teks apa pun dikirim ke model.");
    out
}
/// Teks pemakaian token untuk UI.
pub fn usage_text(
    usage: tui_wire::Usage,
    context_tokens: u64,
    by_model: &[(String, tui_wire::Usage)],
) -> String {
    let mut text = format!(
        "token: input {} · output {} · cache {} · konteks ~{} (perkiraan)",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, context_tokens
    );
    let models: Vec<&(String, tui_wire::Usage)> = by_model
        .iter()
        .filter(|(_, model_usage)| !model_usage.is_zero())
        .collect();
    if models.len() > 1 {
        text.push_str("\nper model:");
        for (name, model_usage) in models.iter().take(5) {
            text.push_str(&format!(
                "\n  {name}: input {} · output {}",
                model_usage.input_tokens, model_usage.output_tokens
            ));
        }
    }
    text
}

/// Teks hasil compaction manual untuk UI.
///
/// Ditulis ASCII (`\u{2192}`) supaya berkas tetap aman dari alat yang menulis
/// ulang dengan encoding sistem.
pub fn compaction_text(report: crate::agent::CompactionReport) -> String {
    format!(
        "konteks diringkas: ~{} \u{2192} ~{} token ({} pesan diganti ringkasan)",
        report.before_tokens, report.after_tokens, report.summarized
    )
}

/// Teks pemakaian konteks terhadap anggaran.
pub fn context_text(used: u64, budget: tui_context::Budget) -> String {
    let usable = budget.usable().max(1);
    let percent = used.saturating_mul(100) / usable;
    format!(
        "konteks ~{} dari {} token ({percent}%) · reserve {} · target pemulihan {} (perkiraan)",
        used,
        usable,
        budget.reserve_tokens,
        budget.recovery_target()
    )
}

/// Teks daftar todo sesi.
pub fn todo_text(items: &[String]) -> String {
    if items.is_empty() {
        return "belum ada todo".to_string();
    }
    let mut text = String::from("todo:");
    for (index, item) in items.iter().enumerate() {
        text.push_str(&format!("\n  {}. {item}", index + 1));
    }
    text
}

/// Cocokkan argumen `/model` dengan daftar model yang diketahui.
///
/// Bila daftar kosong (discovery belum pernah berhasil), argumen diterima apa
/// adanya karena tidak ada dasar untuk menolaknya. Bila daftar ada, pencocokan
/// persis (tanpa peduli huruf besar) atau awalan unik yang dipakai; selain itu
/// galat jelas berisi pilihan yang tersedia.
pub fn resolve_model(argument: &str, known: &[String]) -> Result<String, String> {
    if known.is_empty() {
        return Ok(argument.to_string());
    }
    let lowered = argument.to_ascii_lowercase();
    if let Some(exact) = known
        .iter()
        .find(|model| model.to_ascii_lowercase() == lowered)
    {
        return Ok(exact.clone());
    }
    let matches: Vec<&String> = known
        .iter()
        .filter(|model| model.to_ascii_lowercase().contains(&lowered))
        .collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(format!(
            "model '{argument}' tidak ada di daftar gateway; pilihan: {}",
            known.join(", ")
        )),
        several => Err(format!(
            "'{argument}' cocok dengan beberapa model ({}); pakai id yang lebih panjang",
            several
                .iter()
                .take(5)
                .map(|model| model.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Jalankan satu baris input.
pub fn dispatch(input: &str, ctx: &CommandContext<'_>) -> CommandResult {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return CommandResult::NotACommand;
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let raw_name = parts.next().unwrap_or_default().to_ascii_lowercase();
    let argument = parts.next().unwrap_or_default().trim().to_string();
    let spec = find_command(&raw_name);
    if let Some(spec) = spec {
        if !spec.allow_args && !argument.is_empty() {
            return CommandResult::Reply(format!("{} tidak menerima argumen", spec.display()));
        }
    }
    // Alias dipetakan ke nama kanonik supaya perilakunya benar-benar satu.
    let name = spec.map_or(raw_name.clone(), |spec| spec.name.to_string());
    match name.as_str() {
        "provider" => {
            if argument.is_empty() {
                let mut text = format!(
                    "provider aktif: {} (kunci: {})",
                    ctx.provider.0, ctx.provider.1
                );
                text.push_str("\ntersedia:");
                for (id, label, bawaan) in tui_wire::providers::listing() {
                    let tanda = if bawaan { " [bawaan]" } else { "" };
                    text.push_str(&format!("\n  {id} — {label}{tanda}"));
                }
                CommandResult::Reply(text)
            } else {
                match tui_wire::providers::by_id(&argument) {
                    Some(entry) => CommandResult::SetProvider(Some(entry.id)),
                    None => {
                        let tersedia: Vec<String> = tui_wire::providers::all()
                            .iter()
                            .map(|p| p.id.clone())
                            .collect();
                        CommandResult::Reply(format!(
                            "provider '{argument}' tidak ada di registry; pilihan: {}",
                            tersedia.join(", ")
                        ))
                    }
                }
            }
        }
        "help" => CommandResult::Reply(help_text()),
        "clear" | "cls" => CommandResult::ClearScreen,
        "skills" => CommandResult::Reply(if ctx.skills_ringkas.is_empty() {
            "tidak ada skill aktif (buat folder skills/<nama>/SKILL.md)".to_string()
        } else {
            ctx.skills_ringkas.to_string()
        }),
        "effort" => match argument.as_str() {
            "" => CommandResult::Reply(match ctx.effort {
                Some(e) => format!("effort aktif: {e}"),
                None => "effort: default gateway".to_string(),
            }),
            "low" | "medium" | "high" => CommandResult::SetEffort(Some(argument)),
            lain => CommandResult::Reply(format!(
                "effort '{lain}' tidak dikenal; pilihan: low, medium, high (atau kosong = default)"
            )),
        },
        "extended-context" | "extctx" => match argument.as_str() {
            "on" => CommandResult::SetContextWindow(Some(400_000)),
            "off" => CommandResult::SetContextWindow(ctx.context_window),
            "" => CommandResult::Reply(match ctx.context_window {
                Some(w) => format!(
                    "window konteks config: {w} token (/extended-context on = pakai 400.000)"
                ),
                None => "window konteks tidak dibatasi config".to_string(),
            }),
            lain => CommandResult::Reply(format!("argumen '{lain}' tidak dikenal; pakai on / off")),
        },
        "force" | "yolo" => CommandResult::ForceOnce,
        "model" => {
            if argument.is_empty() {
                let mut text = format!("model aktif: {}", ctx.model);
                if ctx.known_models.is_empty() {
                    text.push_str("\n(daftar model belum diambil dari gateway)");
                } else {
                    text.push_str("\ntersedia: ");
                    text.push_str(&ctx.known_models.join(", "));
                }
                CommandResult::Reply(text)
            } else {
                match resolve_model(&argument, ctx.known_models) {
                    Ok(model) => CommandResult::SetModel(model),
                    Err(message) => CommandResult::Reply(message),
                }
            }
        }
        "compact" => CommandResult::Compact,
        "usage" => CommandResult::Usage,
        "sessions" => CommandResult::Sessions(if argument.is_empty() {
            None
        } else {
            Some(argument)
        }),
        "tree" => CommandResult::Tree,
        "new" => CommandResult::NewSession,
        "fork" => CommandResult::ForkSession,
        "context" => CommandResult::Context,
        "todo" => CommandResult::Todo,
        "title" => {
            if argument.is_empty() {
                CommandResult::Reply("pakai /title <teks>".to_string())
            } else {
                CommandResult::SetTitle(argument)
            }
        }
        "switch" => CommandResult::SwitchSession(if argument.is_empty() {
            None
        } else {
            Some(argument)
        }),
        "forget-approvals" => CommandResult::ForgetApprovals,
        "pause" => CommandResult::Pause,
        "resume" => CommandResult::Resume,
        "tools" => {
            if ctx.tools.is_empty() {
                CommandResult::Reply("belum ada tool terdaftar".to_string())
            } else {
                CommandResult::Reply(ctx.tools.join(", "))
            }
        }
        "session" => CommandResult::Reply(match ctx.session_path {
            Some(path) => path.display().to_string(),
            None => "belum ada file sesi".to_string(),
        }),
        "quit" => CommandResult::Quit,
        other => {
            let names: Vec<String> = COMMANDS.iter().map(CommandSpec::display).collect();
            CommandResult::Unknown(format!(
                "perintah tidak dikenal: /{other}. Tersedia: {}",
                names.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx<'a>(
        model: &'a str,
        tools: &'a [String],
        session: Option<&'a Path>,
    ) -> CommandContext<'a> {
        CommandContext {
            model,
            provider: ("modeluxai", "MODELUXAI_API_KEY"),
            tools,
            session_path: session,
            known_models: &[],
            sessions_dir: None,
            skills_ringkas: "",
            context_window: None,
            effort: None,
        }
    }

    #[test]
    fn teks_biasa_bukan_command() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("halo dunia", &ctx("m", &tools, None)),
            CommandResult::NotACommand
        );
    }

    #[test]
    fn help_menyebut_semua_command() {
        let tools = Vec::new();
        match dispatch("/help", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                for spec in COMMANDS {
                    let display = spec.display();
                    assert!(text.contains(&display), "help tidak menyebut {display}");
                }
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn model_tanpa_argumen_menampilkan_yang_aktif() {
        let tools = Vec::new();
        match dispatch("/model", &ctx("deepseek-v4.1-flash", &tools, None)) {
            CommandResult::Reply(text) => assert!(text.contains("deepseek-v4.1-flash")),
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn model_dengan_argumen_mengganti() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("  /model  glm-5-air ", &ctx("m", &tools, None)),
            CommandResult::SetModel("glm-5-air".to_string())
        );
    }

    #[test]
    fn tools_dan_session_membaca_konteks() {
        let tools = vec!["read".to_string(), "bash".to_string()];
        match dispatch("/tools", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => assert_eq!(text, "read, bash"),
            other => panic!("dapat {other:?}"),
        }
        let path = PathBuf::from("sesi.jsonl");
        match dispatch("/session", &ctx("m", &tools, Some(&path))) {
            CommandResult::Reply(text) => assert!(text.ends_with("sesi.jsonl")),
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn compact_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/compact", &ctx("m", &tools, None)),
            CommandResult::Compact
        );
    }

    #[test]
    fn compaction_menyebut_token_dan_jumlah_pesan() {
        let text = compaction_text(crate::agent::CompactionReport {
            before_tokens: 12_000,
            after_tokens: 3_500,
            summarized: 8,
        });
        assert!(text.contains("12000"), "{text}");
        assert!(text.contains("3500"), "{text}");
        assert!(text.contains("8 pesan"), "{text}");
        assert!(
            text.contains("\u{2192}"),
            "panah ditulis sebagai escape: {text}"
        );
    }

    #[test]
    fn usage_dikenali_dan_diformat() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/usage", &ctx("m", &tools, None)),
            CommandResult::Usage
        );
        let usage = tui_wire::Usage {
            input_tokens: 120,
            output_tokens: 30,
            cached_tokens: 40,
        };
        let text = usage_text(usage, 500, &[]);
        assert!(text.contains("input 120"), "{text}");
        assert!(text.contains("output 30"), "{text}");
        assert!(text.contains("cache 40"), "{text}");
        assert!(text.contains("~500"), "{text}");
        assert!(
            !text.contains("per model"),
            "satu model tidak perlu rincian"
        );
    }

    #[test]
    fn usage_merinci_per_model_bila_lebih_dari_satu() {
        let total = tui_wire::Usage {
            input_tokens: 15,
            output_tokens: 9,
            cached_tokens: 0,
        };
        let by_model = vec![
            (
                "model-a".to_string(),
                tui_wire::Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cached_tokens: 0,
                },
            ),
            (
                "model-b".to_string(),
                tui_wire::Usage {
                    input_tokens: 5,
                    output_tokens: 4,
                    cached_tokens: 0,
                },
            ),
        ];
        let text = usage_text(total, 900, &by_model);
        assert!(text.contains("per model"), "{text}");
        assert!(text.contains("model-a: input 10"), "{text}");
        assert!(text.contains("model-b: input 5"), "{text}");
    }

    fn ctx_models<'a>(known: &'a [String]) -> CommandContext<'a> {
        CommandContext {
            model: "aktif",
            provider: ("modeluxai", "MODELUXAI_API_KEY"),
            tools: &[],
            session_path: None,
            known_models: known,
            sessions_dir: None,
            skills_ringkas: "",
            context_window: None,
            effort: None,
        }
    }

    #[test]
    fn model_tanpa_arg_menampilkan_daftar_bila_ada() {
        let known = vec!["model-a".to_string(), "model-b".to_string()];
        match dispatch("/model", &ctx_models(&known)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("model aktif: aktif"), "{text}");
                assert!(text.contains("model-a"), "{text}");
                assert!(text.contains("model-b"), "{text}");
            }
            other => panic!("harus Reply, dapat {other:?}"),
        }
        match dispatch("/model", &ctx_models(&[])) {
            CommandResult::Reply(text) => {
                assert!(text.contains("belum diambil dari gateway"), "{text}");
            }
            other => panic!("harus Reply, dapat {other:?}"),
        }
    }

    #[test]
    fn model_tidak_dikenal_ditolak_tanpa_menebak() {
        let known = vec!["deepseek-v4.1-flash".to_string()];
        assert_eq!(
            resolve_model("deepseek-v4.1-flash", &known),
            Ok("deepseek-v4.1-flash".to_string())
        );
        assert_eq!(
            resolve_model("DEEPSEEK-v4.1-FLASH", &known),
            Ok("deepseek-v4.1-flash".to_string())
        );
        assert!(
            resolve_model("v4.1", &known).is_ok(),
            "awalan unik diterima"
        );
        let err = resolve_model("model-tak-ada", &known).expect_err("harus ditolak");
        assert!(err.contains("tidak ada di daftar gateway"), "{err}");
        assert!(err.contains("deepseek-v4.1-flash"), "{err}");
        assert_eq!(resolve_model("apa-saja", &[]), Ok("apa-saja".to_string()));
    }

    #[test]
    fn model_tidak_dikenal_lewat_dispatch_tidak_mengganti() {
        let known = vec!["deepseek-v4.1-flash".to_string()];
        match dispatch("/model salah", &ctx_models(&known)) {
            CommandResult::Reply(text) => assert!(text.contains("tidak ada"), "{text}"),
            other => panic!("tidak boleh SetModel: {other:?}"),
        }
        assert_eq!(
            dispatch("/model deepseek-v4.1-flash", &ctx_models(&known)),
            CommandResult::SetModel("deepseek-v4.1-flash".to_string())
        );
    }

    #[test]
    fn registry_memiliki_atribut_dan_alias() {
        let help = find_command("help").expect("help ada");
        assert!(help.aliases.contains(&"?"));
        let quit = find_command("exit").expect("alias exit");
        assert_eq!(quit.name, "quit");
        assert!(quit.aliases.contains(&"exit"));
        let model = find_command("model").expect("model ada");
        assert!(model.allow_args);
        assert_eq!(model.display(), "/model [id]");
        let compact = find_command("compact").expect("compact ada");
        assert!(!compact.allow_args);
        assert_eq!(COMMANDS.len(), 24, "jumlah command harus cocok dokumen");
        for spec in COMMANDS {
            assert!(
                !spec.description.is_empty(),
                "{} tanpa deskripsi",
                spec.name
            );
        }
    }

    #[test]
    fn command_tanpa_argumen_menolak_argumen() {
        let tools = Vec::new();
        match dispatch("/compact sekarang", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("tidak menerima argumen"), "{text}")
            }
            other => panic!("tidak boleh eksekusi: {other:?}"),
        }
        match dispatch("/exit", &ctx("m", &tools, None)) {
            CommandResult::Quit => {}
            other => panic!("alias exit harus keluar: {other:?}"),
        }
        match dispatch("/?", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("perintah yang tersedia"), "{text}")
            }
            other => panic!("alias ? harus menampilkan bantuan: {other:?}"),
        }
    }

    #[test]
    fn provider_tanpa_argumen_menampilkan_daftar() {
        let tools = Vec::new();
        match dispatch("/provider", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("provider aktif: modeluxai"), "{text}");
                assert!(text.contains("MODELUXAI_API_KEY"), "{text}");
                assert!(text.contains("openrouter"), "{text}");
                assert!(text.contains("[bawaan]"), "{text}");
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn provider_dengan_argumen_dikenal_menghasilkan_set() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/provider groq", &ctx("m", &tools, None)),
            CommandResult::SetProvider(Some("groq".to_string()))
        );
        match dispatch("/provider bukan-ada", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("tidak ada di registry"), "{text}");
                assert!(text.contains("modeluxai"), "{text}");
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn forget_approvals_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/forget-approvals", &ctx("m", &tools, None)),
            CommandResult::ForgetApprovals
        );
    }

    #[test]
    fn new_dan_fork_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/new", &ctx("m", &tools, None)),
            CommandResult::NewSession
        );
        assert_eq!(
            dispatch("/fork", &ctx("m", &tools, None)),
            CommandResult::ForkSession
        );
    }

    #[test]
    fn context_dan_todo_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/context", &ctx("m", &tools, None)),
            CommandResult::Context
        );
        assert_eq!(
            dispatch("/todo", &ctx("m", &tools, None)),
            CommandResult::Todo
        );
    }

    #[test]
    fn context_dan_todo_diformat() {
        let budget = tui_context::Budget::new(100_000, 16_000, 20_000);
        let text = context_text(45_000, budget);
        assert!(text.contains("45000"), "{text}");
        assert!(text.contains("84000"), "{text}");
        assert!(text.contains("53%"), "{text}");
        assert!(text.contains("target pemulihan 67200"), "{text}");

        assert_eq!(todo_text(&[]), "belum ada todo");
        let text = todo_text(&["satu".to_string(), "dua".to_string()]);
        assert!(text.contains("1. satu"), "{text}");
        assert!(text.contains("2. dua"), "{text}");
    }

    #[test]
    fn title_sesi_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/title kerja malam", &ctx("m", &tools, None)),
            CommandResult::SetTitle("kerja malam".to_string())
        );
        assert_eq!(
            dispatch("/title", &ctx("m", &tools, None)),
            CommandResult::Reply("pakai /title <teks>".to_string())
        );
    }

    #[test]
    fn switch_sesi_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/switch", &ctx("m", &tools, None)),
            CommandResult::SwitchSession(None)
        );
        assert_eq!(
            dispatch("/switch abc123", &ctx("m", &tools, None)),
            CommandResult::SwitchSession(Some("abc123".to_string()))
        );
        assert_eq!(
            dispatch("/resume", &ctx("m", &tools, None)),
            CommandResult::Resume,
            "/resume tetap untuk melanjutkan sesi yang dijeda"
        );
    }

    #[test]
    fn sessions_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/sessions", &ctx("m", &tools, None)),
            CommandResult::Sessions(None)
        );
        assert_eq!(
            dispatch("/sessions catatan", &ctx("m", &tools, None)),
            CommandResult::Sessions(Some("catatan".to_string()))
        );
    }

    #[test]
    fn pause_dan_resume_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/pause", &ctx("m", &tools, None)),
            CommandResult::Pause
        );
        assert_eq!(
            dispatch("/resume", &ctx("m", &tools, None)),
            CommandResult::Resume
        );
    }

    #[test]
    fn clear_skills_effort_extctx_force_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/clear", &ctx("m", &tools, None)),
            CommandResult::ClearScreen
        );
        assert_eq!(
            dispatch("/cls", &ctx("m", &tools, None)),
            CommandResult::ClearScreen
        );
        assert_eq!(
            dispatch("/skills", &ctx("m", &tools, None)),
            CommandResult::Reply(
                "tidak ada skill aktif (buat folder skills/<nama>/SKILL.md)".to_string()
            )
        );
        assert_eq!(
            dispatch("/effort", &ctx("m", &tools, None)),
            CommandResult::Reply("effort: default gateway".to_string())
        );
        assert_eq!(
            dispatch("/effort high", &ctx("m", &tools, None)),
            CommandResult::SetEffort(Some("high".to_string()))
        );
        assert!(
            matches!(
                dispatch("/effort gila", &ctx("m", &tools, None)),
                CommandResult::Reply(_)
            ),
            "effort tak dikenal = pesan, bukan set"
        );
        assert_eq!(
            dispatch("/extended-context on", &ctx("m", &tools, None)),
            CommandResult::SetContextWindow(Some(400_000))
        );
        assert_eq!(
            dispatch("/extctx off", &ctx("m", &tools, None)),
            CommandResult::SetContextWindow(None)
        );
        assert_eq!(
            dispatch("/force", &ctx("m", &tools, None)),
            CommandResult::ForceOnce
        );
        assert_eq!(
            dispatch("/yolo", &ctx("m", &tools, None)),
            CommandResult::ForceOnce
        );
    }

    #[test]
    fn quit_dan_unknown() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/quit", &ctx("m", &tools, None)),
            CommandResult::Quit
        );
        assert_eq!(
            dispatch("/exit", &ctx("m", &tools, None)),
            CommandResult::Quit
        );
        match dispatch("/tidak-ada", &ctx("m", &tools, None)) {
            CommandResult::Unknown(text) => assert!(text.contains("/help")),
            other => panic!("dapat {other:?}"),
        }
    }
}
