//! Eksekusi command fitur yang butuh sesi, store, config, atau proses OS.
//!
//! Command murni (tanpa I/O) tetap di `tui-core::commands`; modul ini adalah
//! lapisan efek samping yang dipanggil mode TUI/teks setelah `dispatch`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tui_config::ModelPreset;
use tui_core::{AgentSession, TurnSettings};
use tui_session::{SessionEntry, SessionStore};
use tui_wire::Message;

use crate::Boot;

/// Hasil satu command fitur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureAction {
    /// Baris yang ditampilkan ke pengguna.
    Lines(Vec<String>),
    /// Mode harus membuat sesi baru (tanpa membawa grant sesi lama).
    Fresh,
}

/// State fitur yang hidup selama satu sesi mode.
pub struct FeatureState {
    /// Apakah daftar skill ikut di system prompt.
    pub skills_on: bool,
    /// Apakah review advisor aktif.
    pub advisor_on: bool,
    /// Model advisor (kosong = model sesi).
    pub advisor_model: String,
    /// Klien provider advisor terpisah (kosong = pakai klien sesi).
    pub advisor_provider: Option<(String, String)>,
    /// Relay collab yang dijalankan command (dimatikan saat drop).
    pub relay: Option<RelayChild>,
    /// Klien guest relay read-only (bila `/collab join` dipakai).
    pub guest: Option<tokio::task::JoinHandle<()>>,
    /// Kanal event untuk meneruskan siaran guest ke timeline.
    pub events: Option<tokio::sync::mpsc::UnboundedSender<tui_wire::AgentEvent>>,
    /// Daftar todo sesi (dari ToolRegistry); `None` = mode tanpa todo.
    pub todo: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
    /// Root tambahan untuk `glob`/`grep` (`/add-dir`).
    pub extra_dirs: Vec<std::path::PathBuf>,
}

/// Bangun klien advisor bila `[advisor] provider` diisi dan berbeda.
fn advisor_provider(boot: &Boot) -> Option<(String, String)> {
    let provider = boot.config.advisor.provider.trim();
    if provider.is_empty() || provider == boot.provider.0 {
        return None;
    }
    let entry = tui_wire::providers::by_id(provider)?;
    let env = if boot.config.advisor.api_key_env.trim().is_empty() {
        entry.env_keys.first().cloned().unwrap_or_default()
    } else {
        boot.config.advisor.api_key_env.trim().to_string()
    };
    let base = if entry.base_url.trim().is_empty() {
        boot.config.gateway.base_url.clone()
    } else {
        entry.base_url.clone()
    };
    Some((base, env))
}

impl FeatureState {
    /// State awal dari config.
    pub fn new(boot: &Boot) -> Self {
        Self {
            skills_on: true,
            advisor_on: boot.config.advisor.enabled,
            advisor_model: boot.config.advisor.model.clone(),
            advisor_provider: advisor_provider(boot),
            relay: None,
            guest: None,
            events: None,
            todo: None,
            extra_dirs: Vec::new(),
        }
    }
}

/// System prompt dasar + blok skill (dipakai semua mode dan `/skillful`).
pub fn base_system_prompt(boot: &Boot, skills_on: bool) -> String {
    let mut prompt = TurnSettings::default().system_prompt;
    prompt.push_str(&tui_tools::context_files::blok(&boot.context_files));
    prompt.push_str(&crate::context_notes::blok(&boot.context_notes));
    prompt.push_str(&crate::context_notes::annotation_blok(&boot.annotations));
    prompt.push_str(&crate::context_notes::notification_blok(
        &boot.notifications,
    ));
    prompt.push_str(&crate::plan_mode::blok(&boot.plan));
    prompt.push_str(&crate::tool_issues::blok(&boot.tool_issues));
    prompt.push_str(&crate::system_prompt::blok(&boot.system_prompt));
    if skills_on {
        prompt.push_str(&tui_tools::skills::ringkas_daftar(&boot.skills));
    }
    if let Some(memory) = &boot.memory
        && let Ok(block) = memory.prompt_block()
    {
        prompt.push_str(&block);
    }
    prompt
}

/// Jalankan satu command fitur.
pub async fn handle(
    boot: &Boot,
    session: &mut AgentSession,
    store: &mut Option<SessionStore>,
    state: &mut FeatureState,
    name: &str,
    argument: Option<&str>,
) -> FeatureAction {
    let arg = argument.unwrap_or("").trim();
    match name {
        "fresh" => FeatureAction::Fresh,
        "review" => FeatureAction::Lines(review_diff(boot, state, arg == "save").await),
        "annotate-add" => FeatureAction::Lines(annotate_tambah(arg)),
        "annotate-list" => FeatureAction::Lines(annotate_daftar()),
        "annotate-clear" => FeatureAction::Lines(annotate_bersihkan()),
        "hook-test" => FeatureAction::Lines(hook_test(boot, arg).await),
        "artifact" => FeatureAction::Lines(artifact(boot, arg)),
        "conflict" => FeatureAction::Lines(conflict(boot, arg)),
        "xd" => FeatureAction::Lines(xd(arg)),
        "tool-issue" => FeatureAction::Lines(tool_issue(boot, session, arg)),
        "tool-issues" => FeatureAction::Lines(tool_issues_daftar()),
        "tool-issue-resolve" => FeatureAction::Lines(tool_issue_selesai(boot, session, arg)),
        "task-worktree" => FeatureAction::Lines(task_worktree(boot, arg).await),
        "skill-new" => FeatureAction::Lines(skill_baru(&boot.cwd, arg)),
        "skill-rm" => FeatureAction::Lines(skill_hapus(&boot.cwd, arg)),
        "skill-list" => FeatureAction::Lines(skill_daftar(&boot.cwd)),
        "diagnose" => FeatureAction::Lines(diagnose(boot, session)),
        "compare" => FeatureAction::Lines(compare(boot, arg)),
        "watchdog" => FeatureAction::Lines(watchdog(session, arg)),
        "prewalk" => FeatureAction::Lines(prewalk(session, arg)),
        "research" => FeatureAction::Lines(research(session, arg)),
        "guided-goal" => FeatureAction::Lines(goal(boot, session, arg)),
        "think" | "yield" => FeatureAction::Lines(thought(session, name, arg)),
        "goal" => FeatureAction::Lines(goal(boot, session, arg)),
        "loop" => FeatureAction::Lines(loop_goal(session, arg)),
        "ratchet" => FeatureAction::Lines(ratchet(session, arg)),
        "accept" => FeatureAction::Lines(staging_accept(boot, arg)),
        "staging" => FeatureAction::Lines(staging_daftar()),
        "system-prompt" => FeatureAction::Lines(system_prompt(boot, session, arg)),
        "plan" => FeatureAction::Lines(plan(boot, session, arg)),
        "plan-review" => FeatureAction::Lines(plan_review()),
        "install-id" => FeatureAction::Lines(install_id()),
        "prompts" => FeatureAction::Lines(prompts(boot, session, arg)),
        "notifications-add" => FeatureAction::Lines(notifikasi_tambah(arg)),
        "notifications-list" => FeatureAction::Lines(notifikasi_daftar()),
        "notifications-clear" => FeatureAction::Lines(notifikasi_bersihkan()),
        "budget" => FeatureAction::Lines(budget(session, boot)),
        "edit" => FeatureAction::Lines(edit_eksternal(boot, arg).await),
        "add-dir" => FeatureAction::Lines(tambah_dir(boot, session, state, arg)),
        "dirs" => FeatureAction::Lines(daftar_dir(boot, session, state, arg)),
        "checkpoint-list" => FeatureAction::Lines(checkpoint_daftar()),
        "checkpoint" => FeatureAction::Lines(checkpoint_buat(&boot.cwd, session, arg).await),
        "rewind" => FeatureAction::Lines(rewind_ke(&boot.cwd, session, store, arg).await),
        "context-note-add" => FeatureAction::Lines(context_note_tambah(arg)),
        "context-note-list" => FeatureAction::Lines(context_note_daftar()),
        "context-note-clear" => FeatureAction::Lines(context_note_bersihkan()),
        "new-context" => FeatureAction::Lines(vec![
            "catatan konteks dipertahankan; ketik /fresh untuk sesi baru".to_string(),
        ]),
        "pin" => FeatureAction::Lines(pin(session, store, arg, true).await),
        "unpin" => FeatureAction::Lines(pin(session, store, arg, false).await),
        "jobs" => FeatureAction::Lines(jobs(boot)),
        "git" => FeatureAction::Lines(git_status(&boot.cwd).await),
        "local" => FeatureAction::Lines(local(boot, session, state, store)),
        "browser" => FeatureAction::Lines(open_target(&boot.cwd, arg, true)),
        "open" => FeatureAction::Lines(open_target(&boot.cwd, arg, false)),
        "copy" => FeatureAction::Lines(copy(arg)),
        "trace" => FeatureAction::Lines(write_trace(boot, session)),
        "dump" => FeatureAction::Lines(write_dump(boot, session)),
        "debug" => FeatureAction::Lines(write_debug(boot, session)),
        "elide" => FeatureAction::Lines(elide(session, arg)),
        "shake" => FeatureAction::Lines(shake(session, arg)),
        "modelpreset" | "preset" => FeatureAction::Lines(preset(boot, session, store, arg).await),
        "skillful" => FeatureAction::Lines(skillful(boot, session, state, arg)),
        "collab" | "live" => FeatureAction::Lines(relay(boot, state, arg)),
        "fast" => {
            session.set_service_tier(Some("priority".to_string()));
            FeatureAction::Lines(vec![
                "tier layanan: priority (dikirim sebagai service_tier)".to_string(),
            ])
        }
        "slow" => {
            session.set_service_tier(None);
            FeatureAction::Lines(vec!["tier layanan: default".to_string()])
        }
        "branch" => FeatureAction::Lines(branch(boot, store, arg).await),
        "advisor" => FeatureAction::Lines(advisor(boot, state, arg)),
        "record" => FeatureAction::Lines(record(boot, arg)),
        "delete" => FeatureAction::Lines(delete_session(boot, arg).await),
        "export-anon" => FeatureAction::Lines(export_anon(boot, session, store).await),
        "export-zip" => FeatureAction::Lines(export_zip(boot, session, store)),
        "todo-export" => FeatureAction::Lines(todo_export(state, arg)),
        "todo-import" => FeatureAction::Lines(todo_import(state, arg)),
        "todo-copy" => FeatureAction::Lines(todo_copy(state)),
        "btw" => FeatureAction::Lines(cleaner(session, 16, false)),
        "tan" => FeatureAction::Lines(cleaner(session, 4, false)),
        "omfg" => FeatureAction::Lines(cleaner(session, 0, false)),
        "cleanse" => FeatureAction::Lines(cleaner(session, 0, true)),
        "mcp-reauth" => FeatureAction::Lines(mcp_reauth(boot, arg).await),
        "mcp-add" => FeatureAction::Lines(mcp_add(arg).await),
        "mcp-test" => FeatureAction::Lines(mcp_test(boot, arg).await),
        "mcp-enable" => FeatureAction::Lines(mcp_toggle(boot, arg, true)),
        "mcp-disable" => FeatureAction::Lines(mcp_toggle(boot, arg, false)),
        "memory" => FeatureAction::Lines(memory(boot, arg)),
        "mm" => FeatureAction::Lines(mental(boot, arg)),
        "marketplace" => FeatureAction::Lines(marketplace(boot, arg).await),
        "plugins" => FeatureAction::Lines(plugins(boot, session, arg).await),
        "ssh" => FeatureAction::Lines(ssh(boot, session, arg).await),
        "computer" => FeatureAction::Lines(computer(boot, arg).await),
        other => FeatureAction::Lines(vec![format!("fitur '{other}' tidak dikenal")]),
    }
}

fn now_hex() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

fn timestamp() -> String {
    // Nama berkas memakai detik + nanos agar mudah diurutkan dan unik.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:09}", now.as_secs(), now.subsec_nanos())
}

fn preview(message: &Message) -> String {
    let text = match message {
        Message::System { content } | Message::User { content } => content.clone(),
        Message::ToolResult { content, .. } => content.clone(),
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|block| block.as_text())
            .collect::<Vec<_>>()
            .join(" "),
    };
    let ringkas: String = text.chars().take(120).collect();
    ringkas.replace('\n', " ")
}

async fn pin(
    session: &AgentSession,
    store: &mut Option<SessionStore>,
    arg: &str,
    pinned: bool,
) -> Vec<String> {
    let Some(store) = store.as_mut() else {
        return vec!["sesi belum tersimpan; jalankan satu turn dulu".to_string()];
    };
    if !pinned && arg.is_empty() {
        let sudah = tui_session::pins(store.path());
        if sudah.is_empty() {
            return vec!["tidak ada pesan yang disematkan".to_string()];
        }
        let mut lines = Vec::new();
        for (index, preview) in sudah {
            if let Err(err) = store.append(&SessionEntry::pin(index, false, preview.clone())) {
                lines.push(format!("{}: {}", err.code, err.message));
                continue;
            }
            lines.push(format!("📌 pesan {index} sematan dilepas: {preview}"));
        }
        return lines;
    }
    let total = session.messages().len();
    if total == 0 {
        return vec!["belum ada pesan untuk disematkan".to_string()];
    }
    let index = if arg.is_empty() {
        total
    } else {
        match arg.parse::<usize>() {
            Ok(n) if n >= 1 && n <= total => n,
            Ok(_) => {
                return vec![format!("nomor pesan di luar rentang 1..={total}")];
            }
            Err(_) => return vec![format!("nomor pesan '{arg}' tidak valid")],
        }
    };
    let Some(message) = session.messages().get(index - 1) else {
        return vec![format!("nomor pesan di luar rentang 1..={total}")];
    };
    let ringkas = preview(message);
    let entry = SessionEntry::pin(index as u64, pinned, ringkas.clone());
    if let Err(err) = store.append(&entry) {
        return vec![format!("{}: {}", err.code, err.message)];
    }
    let aksi = if pinned {
        "disematkan"
    } else {
        "sematan dilepas"
    };
    vec![format!("📌 pesan {index} {aksi}: {ringkas}")]
}

fn jobs(boot: &Boot) -> Vec<String> {
    let Some(manager) = boot.subagents.as_ref() else {
        return vec!["mode ini tidak punya manager subagent".to_string()];
    };
    let daftar = manager.list();
    if daftar.is_empty() {
        return vec!["tidak ada job/subagent".to_string()];
    }
    let mut lines = vec![format!("job aktif: {}", daftar.len())];
    for handle in daftar {
        let sesi = handle
            .session_path
            .lock()
            .ok()
            .and_then(|path| path.clone())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "tanpa berkas".to_string());
        lines.push(format!(
            "  {} [{}] induk={} sesi={} (agent://{} / history://{})",
            handle.id,
            handle.status().label(),
            handle.parent_id,
            sesi,
            handle.id,
            handle.id
        ));
    }
    lines
}

async fn git_status(cwd: &Path) -> Vec<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .arg("status")
        .arg("--short")
        .arg("--branch");
    command.kill_on_drop(true);
    let hasil = tokio::time::timeout(Duration::from_secs(10), command.output()).await;
    match hasil {
        Err(_) => vec!["git status melewati batas 10 detik".to_string()],
        Ok(Err(err)) => vec![format!("git tidak bisa dijalankan: {err}")],
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !output.status.success() {
                let detail = if stderr.is_empty() { stdout } else { stderr };
                return vec![format!("git status gagal ({}): {detail}", output.status)];
            }
            let mut text = stdout;
            if text.is_empty() {
                text = "(bersih)".to_string();
            }
            text.lines().map(str::to_string).collect()
        }
    }
}

fn local(
    boot: &Boot,
    session: &AgentSession,
    state: &FeatureState,
    store: &Option<SessionStore>,
) -> Vec<String> {
    vec![
        format!("model: {}", session.model()),
        format!("provider: {} (kunci: {})", boot.provider.0, boot.provider.1),
        format!("effort: {}", session.effort().unwrap_or("default")),
        format!(
            "service tier: {}",
            session.service_tier().unwrap_or("default")
        ),
        format!(
            "konteks: {} pesan, ~{} token, usable {}",
            session.messages().len(),
            session.estimated_tokens(),
            session.budget().usable()
        ),
        format!("approval: {:?}", boot.approval.mode),
        format!("skills di prompt: {}", state.skills_on),
        format!("memory backend: {}", config_sekarang(boot).memory.backend),
        format!("advisor: {}", if state.advisor_on { "on" } else { "off" }),
        format!(
            "sesi: {}",
            store
                .as_ref()
                .map(|s| s.path().display().to_string())
                .unwrap_or_else(|| "belum tersimpan".to_string())
        ),
        format!("cwd: {}", boot.cwd.display()),
    ]
}

fn open_target(cwd: &Path, target: &str, browser_only: bool) -> Vec<String> {
    if target.is_empty() {
        return vec!["pakai: /browser <url> atau /open <path|url>".to_string()];
    }
    let is_url = target.starts_with("http://") || target.starts_with("https://");
    if browser_only && !is_url {
        return vec!["/browser hanya menerima URL http(s)".to_string()];
    }
    let buka = if is_url {
        target.to_string()
    } else {
        let path = if Path::new(target).is_absolute() {
            PathBuf::from(target)
        } else {
            cwd.join(target)
        };
        if !path.exists() {
            return vec![format!("path tidak ditemukan: {}", path.display())];
        }
        path.display().to_string()
    };
    match open_system(&buka) {
        Ok(()) => vec![format!("dibuka: {buka}")],
        Err(err) => vec![format!("gagal membuka {buka}: {err}")],
    }
}

fn open_system(target: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(target);
        c
    };
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec!["pakai: /copy <teks>".to_string()];
    }
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => vec![format!(
            "disalin ke clipboard ({} karakter)",
            text.chars().count()
        )],
        Err(err) => vec![format!("clipboard tidak tersedia: {err}")],
    }
}

fn write_json(prefix: &str, value: &serde_json::Value) -> Result<PathBuf, String> {
    let dir = crate::config_root().join("debug");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{prefix}-{}.json", timestamp()));
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let _ = tui_wire::secure_file::restrict_to_current_user(&path);
    Ok(path)
}

fn write_trace(_boot: &Boot, session: &AgentSession) -> Vec<String> {
    let Some(request) = session.last_request() else {
        return vec!["belum ada permintaan model yang tercatat".to_string()];
    };
    match write_json("trace", request) {
        Ok(path) => vec![format!("trace ditulis: {}", path.display())],
        Err(err) => vec![format!("trace gagal: {err}")],
    }
}

fn dump_value(session: &AgentSession) -> serde_json::Value {
    let mut snapshot = session.debug_snapshot();
    if let Some(object) = snapshot.as_object_mut() {
        object.insert(
            "messages_detail".to_string(),
            serde_json::to_value(session.messages()).unwrap_or(serde_json::Value::Null),
        );
    }
    snapshot
}

fn write_dump(_boot: &Boot, session: &AgentSession) -> Vec<String> {
    match write_json("dump", &dump_value(session)) {
        Ok(path) => vec![format!("dump ditulis: {}", path.display())],
        Err(err) => vec![format!("dump gagal: {err}")],
    }
}

fn write_debug(_boot: &Boot, session: &AgentSession) -> Vec<String> {
    let mut value = dump_value(session);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "last_request".to_string(),
            session
                .last_request()
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
    }
    match write_json("debug", &value) {
        Ok(path) => vec![format!("debug ditulis: {}", path.display())],
        Err(err) => vec![format!("debug gagal: {err}")],
    }
}

fn elide(session: &mut AgentSession, arg: &str) -> Vec<String> {
    let keep = if arg.is_empty() {
        0
    } else {
        match arg.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return vec![format!("keep '{arg}' bukan angka")],
        }
    };
    let pruned = session.apply_elide(keep);
    vec![format!(
        "elide: {pruned} hasil tool dipangkas (keep {keep})"
    )]
}

fn shake(session: &mut AgentSession, arg: &str) -> Vec<String> {
    match arg {
        "thinking" => {
            let removed = session.apply_strip_thinking();
            vec![format!("shake thinking: {removed} blok penalaran dibuang")]
        }
        "images" | "image" => vec![
            "shake images: skema pesan v1 tidak menyimpan blok gambar; tidak ada yang dipangkas"
                .to_string(),
        ],
        "" | "semua" | "all" => {
            let target = session.budget().usable() / 2;
            match session.apply_shake(target, 8) {
                Some(report) => vec![format!(
                    "shake: {} hasil tool dipangkas, ~{} → ~{} token",
                    report.pruned, report.before_tokens, report.after_tokens
                )],
                None => vec!["shake: konteks sudah kecil atau tidak ada hasil tool".to_string()],
            }
        }
        lain => vec![format!(
            "varian shake '{lain}' tidak dikenal; pakai thinking, images, atau semua"
        )],
    }
}

async fn preset(
    boot: &Boot,
    session: &mut AgentSession,
    store: &mut Option<SessionStore>,
    arg: &str,
) -> Vec<String> {
    let path = global_config_path();
    let config = config_sekarang(boot);
    let (sub, name) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let name = name.trim();
    match sub {
        "" | "list" => {
            if config.presets.is_empty() {
                return vec!["belum ada preset".to_string()];
            }
            let mut lines = vec!["preset:".to_string()];
            for (nama, preset) in &config.presets {
                lines.push(format!(
                    "  {nama}: provider={} model={} effort={}",
                    if preset.provider.is_empty() {
                        "-"
                    } else {
                        preset.provider.as_str()
                    },
                    if preset.model.is_empty() {
                        "-"
                    } else {
                        preset.model.as_str()
                    },
                    if preset.effort.is_empty() {
                        "-"
                    } else {
                        preset.effort.as_str()
                    }
                ));
            }
            lines
        }
        "save" => {
            if name.is_empty() {
                return vec!["pakai: /modelpreset save <nama>".to_string()];
            }
            let preset = ModelPreset {
                provider: boot.provider.0.clone(),
                model: session.model().to_string(),
                effort: session.effort().unwrap_or_default().to_string(),
            };
            match tui_config::set_global_preset(&path, name, &preset) {
                Ok(()) => vec![format!("preset '{name}' disimpan")],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "use" => {
            let Some(preset) = config.presets.get(name) else {
                return vec![format!("preset '{name}' tidak ada")];
            };
            if !preset.model.is_empty() {
                session.set_model(preset.model.clone());
                crate::session_open::append(
                    store,
                    SessionEntry::model_change(preset.model.clone()),
                )
                .await;
            }
            if !preset.effort.is_empty() {
                session.set_effort(Some(preset.effort.clone()));
            }
            let mut lines = vec![format!("preset '{name}' diterapkan: {}", preset.model)];
            if !preset.provider.is_empty() && preset.provider != boot.provider.0 {
                lines.push(format!(
                    "provider preset '{}' tidak diganti otomatis; jalankan /provider {}",
                    preset.provider, preset.provider
                ));
            }
            lines
        }
        "rm" | "remove" => match tui_config::remove_global_value(&path, &format!("presets.{name}"))
        {
            Ok(true) => vec![format!("preset '{name}' dihapus")],
            Ok(false) => vec![format!("preset '{name}' tidak ada")],
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        lain => vec![format!(
            "subcommand preset '{lain}' tidak dikenal; pakai list, save, use, rm"
        )],
    }
}

fn global_config_path() -> PathBuf {
    tui_config::default_global_path().unwrap_or_else(|| crate::config_root().join("config.toml"))
}

/// Muat config terbaru dari disk (agar save/use langsung terlihat).
fn config_sekarang(boot: &Boot) -> tui_config::Config {
    let global = tui_config::default_global_path();
    let project = tui_config::default_project_path(&boot.cwd);
    tui_config::load(global.as_deref(), Some(&project))
        .map(|loaded| loaded.config)
        .unwrap_or_else(|_| boot.config.clone())
}

fn skillful(
    boot: &Boot,
    session: &mut AgentSession,
    state: &mut FeatureState,
    arg: &str,
) -> Vec<String> {
    match arg {
        "" => vec![format!(
            "skillful: {} (pakai on/off)",
            if state.skills_on { "on" } else { "off" }
        )],
        "on" => {
            state.skills_on = true;
            session.set_system_prompt(base_system_prompt(boot, true));
            vec!["skillful: on (daftar skill masuk system prompt)".to_string()]
        }
        "off" => {
            state.skills_on = false;
            session.set_system_prompt(base_system_prompt(boot, false));
            vec!["skillful: off (daftar skill tidak dikirim)".to_string()]
        }
        lain => vec![format!("argumen '{lain}' tidak dikenal; pakai on/off")],
    }
}

fn relay(boot: &Boot, state: &mut FeatureState, arg: &str) -> Vec<String> {
    let sub = if arg.is_empty() { "start" } else { arg };
    match sub {
        "join" | "join " => vec![format!(
            "pakai: /collab join <room> <kunci>{}",
            if boot.config.collab.remote_url.is_empty() {
                " [atau: /collab join <ws-url> <room> <kunci>]"
            } else {
                " (memakai collab.remote_url dari config)"
            }
        )],
        _ if sub.starts_with("join ") => {
            let bagian: Vec<&str> = sub[5..].split_whitespace().collect();
            let parsed = if bagian
                .first()
                .is_some_and(|u| u.starts_with("ws://") || u.starts_with("wss://"))
            {
                guest_join_parts(sub)
            } else if bagian.len() == 2 && !boot.config.collab.remote_url.is_empty() {
                Some((
                    boot.config.collab.remote_url.clone(),
                    bagian[0].to_string(),
                    bagian[1].to_string(),
                ))
            } else {
                None
            };
            let Some((url, room, kunci)) = parsed else {
                return vec![
                    "pakai: /collab join <room> <kunci> atau <ws-url> <room> <kunci>".to_string(),
                ];
            };
            let Some(events) = state.events.clone() else {
                return vec!["mode ini tidak punya kanal event untuk guest".to_string()];
            };
            if let Some(lama) = state.guest.take() {
                lama.abort();
            }
            state.guest = Some(spawn_relay_guest(url.clone(), room.clone(), kunci, events));
            vec![format!(
                "guest read-only tersambung ke {url} room '{room}' (ketik /collab stop untuk putus)"
            )]
        }
        "start" => {
            if let Some(child) = state.relay.as_mut() {
                return vec![format!("relay sudah berjalan: {}", child.summary())];
            }
            match RelayChild::start(boot.config.collab.port) {
                Ok(mut child) => {
                    let lines = vec![
                        format!("relay berjalan: {}", child.summary()),
                        format!("log: {}", child.log.display()),
                        "hentikan dengan /collab stop".to_string(),
                    ];
                    state.relay = Some(child);
                    lines
                }
                Err(err) => vec![format!("relay gagal start: {err}")],
            }
        }
        "status" => {
            let mut lines = Vec::new();
            match state.relay.as_mut() {
                Some(child) => lines.push(child.summary()),
                None => lines.push("relay tidak berjalan".to_string()),
            }
            lines.push(format!(
                "guest: {}",
                if state.guest.is_some() {
                    "aktif (read-only)"
                } else {
                    "tidak aktif"
                }
            ));
            lines
        }
        "stop" => {
            let mut lines = Vec::new();
            match state.relay.take() {
                Some(mut child) => {
                    child.stop();
                    lines.push("relay dihentikan".to_string());
                }
                None => lines.push("relay tidak berjalan".to_string()),
            }
            if let Some(guest) = state.guest.take() {
                guest.abort();
                lines.push("guest diputus".to_string());
            }
            lines
        }
        lain => vec![format!(
            "subcommand relay '{lain}' tidak dikenal; pakai start, status, stop"
        )],
    }
}

/// Proses relay yang dijalankan command `/collab` / `/live`.
pub struct RelayChild {
    child: std::process::Child,
    port: u16,
    key: String,
    log: PathBuf,
}

impl RelayChild {
    /// Jalankan `maicli --mode relay --port <port>` dengan kunci room baru.
    pub fn start(port: u16) -> Result<Self, String> {
        // Bind test cepat: beri pesan jelas sebelum child mati di latar.
        let probe = std::net::TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("port {port} tidak bisa dipakai: {e}"))?;
        drop(probe);
        let exe = std::env::current_exe().map_err(|e| format!("binary tidak diketahui: {e}"))?;
        let key = format!("maicli-{}-{}-{}", std::process::id(), now_hex(), port);
        let log = crate::config_root().join("relay.log");
        if let Some(parent) = log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let sink = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("{}: {e}", log.display()))?;
        let sink_err = sink.try_clone().map_err(|e| e.to_string())?;
        let child = std::process::Command::new(exe)
            .args(["--mode", "relay", "--port"])
            .arg(port.to_string())
            .env("MAICLI_RELAY_KEY", &key)
            .stdout(std::process::Stdio::from(sink))
            .stderr(std::process::Stdio::from(sink_err))
            .spawn()
            .map_err(|e| format!("spawn relay: {e}"))?;
        Ok(Self {
            child,
            port,
            key,
            log,
        })
    }

    /// Ringkasan untuk pengguna.
    pub fn summary(&mut self) -> String {
        let status = match self.child.try_wait() {
            Ok(Some(status)) => format!("berhenti ({status})"),
            Ok(None) => format!("pid {}", self.child.id()),
            Err(err) => format!("status tidak diketahui: {err}"),
        };
        format!(
            "ws://127.0.0.1:{} (kunci room: {}) [{status}]",
            self.port, self.key
        )
    }

    /// Hentikan proses relay.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for RelayChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn branch(boot: &Boot, store: &mut Option<SessionStore>, title: &str) -> Vec<String> {
    let Some(current) = store.as_ref().map(|s| s.path().to_path_buf()) else {
        return vec![
            "belum ada sesi tersimpan; jalankan satu turn dulu sebelum /branch".to_string(),
        ];
    };
    let (kind, judul) = match title {
        "pane" | "window" | "tab" => (title, "sesi maicli (branch)".to_string()),
        "" => ("", "sesi maicli (branch)".to_string()),
        lain => ("", lain.to_string()),
    };
    match crate::session_open::fork_current(current, boot.sessions_dir.clone(), judul).await {
        Ok((new_store, _messages)) => {
            let id = new_store.session_id().to_string();
            // Lock fork harus dilepas sebelum proses baru membuka sesi yang sama.
            drop(new_store);
            match pane_argv(
                std::env::var("TMUX").ok().as_deref(),
                std::env::var("WEZTERM_PANE").ok().as_deref(),
                std::env::var("KITTY_WINDOW_ID").ok().as_deref(),
                &std::env::current_exe().unwrap_or_default(),
                &id,
                kind,
            ) {
                PaneCommand::Spawn(argv) => match std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(_) => vec![format!("branch {id} dibuka di pane/window baru")],
                    Err(err) => vec![format!(
                        "branch {id} dibuat, tetapi pane gagal dibuka: {err} (jalankan maicli --resume {id})"
                    )],
                },
                PaneCommand::Manual(teks) => vec![
                    format!("branch {id} dibuat; jalankan di terminal lain:"),
                    teks,
                ],
            }
        }
        Err(err) => vec![format!("{}: {}", err.code, err.message)],
    }
}

/// Perintah yang dipakai `/branch` untuk membuka sesi di terminal lain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneCommand {
    /// Argv yang siap dijalankan.
    Spawn(Vec<String>),
    /// Tidak ada multiplexer; tampilkan perintah manual.
    Manual(String),
}

/// Bangun argv pane dari environment multiplexer (murni, mudah diuji).
pub fn pane_argv(
    tmux: Option<&str>,
    wezterm: Option<&str>,
    kitty: Option<&str>,
    exe: &Path,
    session_id: &str,
    kind: &str,
) -> PaneCommand {
    let exe = exe.display().to_string();
    if tmux.is_some() {
        let sub = if kind == "pane" {
            "split-window"
        } else {
            "new-window"
        };
        return PaneCommand::Spawn(vec![
            "tmux".to_string(),
            sub.to_string(),
            "-n".to_string(),
            format!("maicli-{session_id}"),
            "--".to_string(),
            exe,
            "--resume".to_string(),
            session_id.to_string(),
        ]);
    }
    if wezterm.is_some() {
        let sub = match kind {
            "window" | "tab" => "spawn",
            _ => "split-pane",
        };
        return PaneCommand::Spawn(vec![
            "wezterm".to_string(),
            "cli".to_string(),
            sub.to_string(),
            "--".to_string(),
            exe,
            "--resume".to_string(),
            session_id.to_string(),
        ]);
    }
    if kitty.is_some() {
        return PaneCommand::Spawn(vec![
            "kitty".to_string(),
            "@".to_string(),
            "launch".to_string(),
            "--type=tab".to_string(),
            exe,
            "--resume".to_string(),
            session_id.to_string(),
        ]);
    }
    PaneCommand::Manual(format!(
        "{exe} --resume {session_id}  (butuh tmux/wezterm/kitty untuk buka pane otomatis)"
    ))
}

/// Pisahkan argumen `/collab join <url> <room> <kunci>`.
pub fn guest_join_parts(argument: &str) -> Option<(String, String, String)> {
    let rest = argument.strip_prefix("join ")?;
    let mut bagian = rest.split_whitespace();
    let url = bagian.next()?.to_string();
    let room = bagian.next()?.to_string();
    let kunci = bagian.next()?.to_string();
    if !(url.starts_with("ws://") || url.starts_with("wss://")) {
        return None;
    }
    Some((url, room, kunci))
}

/// Hubungkan klien relay **read-only** (`view: true`) dan teruskan event ke timeline.
///
/// Klien ini tidak pernah mengirim prompt/abort; hanya join lalu mendengarkan.
/// Event relay diproyeksikan menjadi `AgentEvent::Notice` agar muncul di TUI/teks.
pub fn spawn_relay_guest(
    url: String,
    room: String,
    key: String,
    events: tokio::sync::mpsc::UnboundedSender<tui_wire::AgentEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as Ws;
        let kirim = |teks: String| {
            let _ = events.send(tui_wire::AgentEvent::Notice { text: teks });
        };
        let (mut ws, _) = match tokio_tungstenite::connect_async(&url).await {
            Ok(ws) => ws,
            Err(err) => {
                kirim(format!("[guest] gagal menyambung ke {url}: {err}"));
                return;
            }
        };
        let join = serde_json::json!({"join": format!("{room}:{key}"), "view": true});
        if ws.send(Ws::text(join.to_string())).await.is_err() {
            kirim("[guest] gagal mengirim frame join".to_string());
            return;
        }
        kirim(format!(
            "[guest] tersambung ke {url} room '{room}' (read-only)"
        ));
        while let Some(pesan) = ws.next().await {
            let Ok(Ws::Text(teks)) = pesan else {
                continue;
            };
            let value: serde_json::Value =
                serde_json::from_str(&teks).unwrap_or(serde_json::Value::Null);
            if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
                kirim(format!("[guest] relay menolak: {error}"));
                break;
            }
            let nama = value
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("event");
            let detail = value
                .get("detail")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let isi = if nama == "jawab" {
                detail
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string()
            } else if nama == "prompt" {
                detail
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string()
            } else {
                serde_json::to_string(&detail).unwrap_or_default()
            };
            kirim(format!("[guest] {nama} {isi}"));
        }
        kirim("[guest] koneksi relay ditutup".to_string());
    })
}

/// Jalankan refresh mental model berkala (`memory.auto_refresh_models`).
///
/// Hanya model yang sudah ada yang di-refresh; task berhenti saat runtime
/// berakhir. Interval minimal 30 detik supaya tidak membebani disk.
pub fn spawn_mental_model_refresh(
    memory: std::sync::Arc<tui_memory::MemoryHandle>,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let jeda = std::time::Duration::from_secs(interval_secs.max(30));
        loop {
            tokio::time::sleep(jeda).await;
            match memory.refresh_all_models() {
                Ok(0) => {}
                Ok(jumlah) => {
                    tracing::info!(
                        target: "maicli::memory",
                        "refresh otomatis {jumlah} mental model"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        target: "maicli::memory",
                        "refresh mental model gagal: {}: {}",
                        err.code,
                        err.message
                    );
                }
            }
        }
    })
}

/// Ringkas satu event sesi anak untuk push RPC `subagent_event`.
pub fn ringkas_event_anak(event: &tui_wire::AgentEvent) -> Option<(&'static str, String)> {
    match event {
        tui_wire::AgentEvent::AgentStart => Some(("agent_start", "{}".to_string())),
        tui_wire::AgentEvent::AgentEnd { turns } => {
            Some(("agent_end", format!(r#"{{"turns":{turns}}}"#)))
        }
        tui_wire::AgentEvent::TurnStart { index } => {
            Some(("turn_start", format!(r#"{{"index":{index}}}"#)))
        }
        tui_wire::AgentEvent::TurnEnd { index } => {
            Some(("turn_end", format!(r#"{{"index":{index}}}"#)))
        }
        tui_wire::AgentEvent::ToolExecutionStart {
            tool_name, preview, ..
        } => Some((
            "tool_start",
            format!(
                r#"{{"tool":{},"preview":{}}}"#,
                serde_json::to_string(tool_name).unwrap_or_else(|_| r#""""#.to_string()),
                serde_json::to_string(preview).unwrap_or_else(|_| r#""""#.to_string())
            ),
        )),
        tui_wire::AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            preview,
            ..
        } => Some((
            "tool_end",
            format!(
                r#"{{"tool":{},"is_error":{is_error},"preview":{}}}"#,
                serde_json::to_string(tool_name).unwrap_or_else(|_| r#""""#.to_string()),
                serde_json::to_string(preview).unwrap_or_else(|_| r#""""#.to_string())
            ),
        )),
        tui_wire::AgentEvent::Notice { text } => Some((
            "notice",
            serde_json::to_string(text).unwrap_or_else(|_| r#""""#.to_string()),
        )),
        _ => None,
    }
}

/// Jalan bila advisor aktif: minta model kedua mereview jawaban terakhir.
pub async fn advisor_review(
    boot: &Boot,
    state: &FeatureState,
    session: &AgentSession,
) -> Option<String> {
    if !state.advisor_on || session.messages().is_empty() {
        return None;
    }
    let model = if state.advisor_model.is_empty() {
        session.model().to_string()
    } else {
        state.advisor_model.clone()
    };
    let mut potong: Vec<Message> = session
        .messages()
        .iter()
        .rev()
        .take(12)
        .rev()
        .cloned()
        .collect();
    potong.push(Message::user(
        "Review jawaban terakhir di atas: sebutkan kesalahan faktual, risiko keamanan, \
         atau langkah yang terlewat. Ringkas maksimal 5 poin; jika sudah baik, tulis 'OK'.",
    ));
    let mut messages = vec![Message::system(
        "Kamu advisor teknis yang skeptis. Fokus pada kesalahan dan risiko, tanpa mengulang jawaban.",
    )];
    messages.extend(potong);
    let request = tui_provider::ChatRequest {
        model,
        messages,
        tools: Vec::new(),
        max_tokens: Some(700),
        reasoning_effort: None,
        service_tier: None,
    };
    // Provider kedua: klien terpisah + kunci env-nya sendiri (bila diisi).
    let receiver = if let Some((base, env)) = &state.advisor_provider {
        let key = std::env::var(env).ok().filter(|v| !v.trim().is_empty());
        match tui_provider::OpenAiCompatClient::new(base.clone(), key) {
            Ok(client) => tui_provider::Provider::stream_chat(&client, request).await,
            Err(err) => return Some(format!("advisor gagal: {}", err.message)),
        }
    } else {
        tui_provider::Provider::stream_chat(&boot.client, request).await
    }
    .map_err(|err| format!("advisor gagal: {}", err.message));
    let mut receiver = match receiver {
        Ok(receiver) => receiver,
        Err(pesan) => return Some(pesan),
    };
    let mut hasil = String::new();
    while let Some(event) = receiver.recv().await {
        match event {
            tui_wire::ProviderEvent::Done { message } => {
                hasil = message.joined_text();
            }
            tui_wire::ProviderEvent::Error { message, .. } => {
                return Some(format!("advisor gagal: {message}"));
            }
            _ => {}
        }
    }
    let ringkas = hasil.trim();
    if ringkas.is_empty() {
        Some("advisor: tidak ada catatan".to_string())
    } else {
        Some(format!("advisor: {ringkas}"))
    }
}

/// Bangun Markdown transkrip sesi; `anonim` meredaksi kredensial & path user.
fn transcript_markdown(session: &AgentSession, anonim: bool) -> String {
    let mut teks = String::from(if anonim {
        "# Transkrip maicli (anonim)\n\n"
    } else {
        "# Transkrip maicli\n\n"
    });
    for message in session.messages() {
        let (label, isi) = match message {
            Message::User { content } => ("kamu", content.clone()),
            Message::Assistant { content, .. } => (
                "maicli",
                content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            Message::ToolResult {
                tool_name, content, ..
            } => ("tool", format!("{tool_name}: {content}")),
            Message::System { .. } => continue,
        };
        let isi = if anonim { redaksi(&isi) } else { isi };
        teks.push_str(&format!("## {label}\n\n{isi}\n\n"));
    }
    teks
}

/// Jalankan hook eksternal untuk satu event (manual test).
async fn hook_test(boot: &Boot, arg: &str) -> Vec<String> {
    let event = arg.trim();
    let daftar = boot.config.hooks.untuk(event);
    if daftar.is_empty() {
        return vec![format!(
            "tidak ada hook untuk '{event}' (event: before_model|before_tool|after_tool|on_stop)"
        )];
    }
    let konteks = serde_json::json!({
        "event": event,
        "cwd": boot.cwd.display().to_string(),
        "model": boot.model,
    })
    .to_string();
    let mut hasil = Vec::new();
    for perintah in daftar {
        match crate::hooks::jalankan(perintah, &konteks).await {
            Ok(teks) => hasil.push(format!("hook '{}': {}", perintah, teks.trim())),
            Err(err) => hasil.push(format!("hook '{}' gagal: {err}", perintah)),
        }
    }
    hasil
}

/// Kelola artefak biner.
fn artifact(boot: &Boot, arg: &str) -> Vec<String> {
    let akar = crate::config_root();
    let (sub, sisa) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let sisa = sisa.trim();
    match sub {
        "add" if !sisa.is_empty() => {
            let sumber = if std::path::Path::new(sisa).is_absolute() {
                std::path::PathBuf::from(sisa)
            } else {
                boot.cwd.join(sisa)
            };
            match crate::artifacts::tambah(&akar, &sumber) {
                Ok(a) => vec![format!(
                    "artefak {} · {} byte · sha256 {}",
                    a.id, a.size, a.sha256
                )],
                Err(err) => vec![format!("gagal menyimpan artefak: {err}")],
            }
        }
        "get" if !sisa.is_empty() => match crate::artifacts::cari(&akar, sisa) {
            Some(a) => vec![format!(
                "{} · {} · {} byte · {}",
                a.id,
                a.nama,
                a.size,
                crate::artifacts::dir(&akar).join(&a.file).display()
            )],
            None => vec![format!("artefak '{sisa}' tidak ditemukan")],
        },
        "list" | "" => {
            let semua = crate::artifacts::daftar(&akar);
            if semua.is_empty() {
                vec!["belum ada artefak".to_string()]
            } else {
                semua
                    .iter()
                    .map(|a| format!("{} · {} · {} byte", a.id, a.nama, a.size))
                    .collect()
            }
        }
        _ => vec!["pakai: /artifact [add <path>|list|get <id>]".to_string()],
    }
}

/// Daftar/selesaikan konflik merge git.
fn conflict(boot: &Boot, arg: &str) -> Vec<String> {
    let daftar = match crate::conflict::daftar(&boot.cwd) {
        Ok(v) => v,
        Err(err) => return vec![format!("gagal membaca konflik: {err}")],
    };
    if daftar.is_empty() {
        return vec!["tidak ada konflik merge".to_string()];
    }
    let bagian: Vec<&str> = arg.split_whitespace().collect();
    if bagian.is_empty() || bagian[0] == "list" {
        return daftar
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{i}. {p} (pakai: /conflict {i} ours|theirs|base)"))
            .collect();
    }
    let Some(indeks) = crate::conflict::parse_uri(bagian[0]) else {
        return vec!["pakai: /conflict N ours|theirs|base".to_string()];
    };
    let Some(path) = daftar.get(indeks) else {
        return vec![format!("indeks {indeks} di luar daftar konflik")];
    };
    let Some(sisi) = bagian.get(1) else {
        return vec![format!("pilih sisi untuk {path}: ours|theirs|base")];
    };
    match crate::conflict::pilih(&boot.cwd, path, sisi) {
        Ok(()) => vec![format!("{path}: memakai {sisi} dan ditandai selesai")],
        Err(err) => vec![format!("gagal menyelesaikan konflik: {err}")],
    }
}

/// Ambil id dari URI `xd://issue/<id>`.
pub fn xd_issue_id(uri: &str) -> Option<&str> {
    uri.strip_prefix("xd://issue/")
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// Resolver URI internal; saat ini hanya isu tool.
fn xd(uri: &str) -> Vec<String> {
    let Some(id) = xd_issue_id(uri) else {
        return vec!["skema xd yang dikenal: xd://issue/<id>".to_string()];
    };
    match crate::tool_issues::muat().into_iter().find(|i| i.id == id) {
        Some(i) => vec![format!(
            "{} [{}] {} ({}) — {}",
            i.id, i.status, i.tool, i.severity, i.pesan
        )],
        None => vec![format!("isu '{id}' tidak ditemukan")],
    }
}

/// Laporkan isu tool baru.
fn tool_issue(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let bagian: Vec<&str> = arg.splitn(3, char::is_whitespace).collect();
    if bagian.len() < 3 {
        return vec!["pakai: /tool-issue <tool> <severity> <pesan>".to_string()];
    }
    match crate::tool_issues::tambah(bagian[0], bagian[2], bagian[1]) {
        Ok(id) => {
            session.set_system_prompt(format!(
                "{}\n\n## Isu tool terbuka (/tool-issues)\n{}",
                base_system_prompt(boot, true),
                crate::tool_issues::blok(&crate::tool_issues::muat())
            ));
            vec![format!("isu {id} dicatat")]
        }
        Err(err) => vec![format!("gagal mencatat isu: {err}")],
    }
}

/// Daftar isu tool.
fn tool_issues_daftar() -> Vec<String> {
    let semua = crate::tool_issues::muat();
    if semua.is_empty() {
        return vec!["belum ada isu tool".to_string()];
    }
    semua
        .iter()
        .map(|i| {
            format!(
                "{} [{}] {} ({}) — {}",
                i.id, i.status, i.tool, i.severity, i.pesan
            )
        })
        .collect()
}

/// Tandai isu tool selesai.
fn tool_issue_selesai(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let id = arg.trim();
    if id.is_empty() {
        return vec!["pakai: /tool-issue-resolve <id>".to_string()];
    }
    match crate::tool_issues::selesaikan(id) {
        Ok(true) => {
            session.set_system_prompt(format!(
                "{}\n\n## Isu tool terbuka (/tool-issues)\n{}",
                base_system_prompt(boot, true),
                crate::tool_issues::blok(&crate::tool_issues::muat())
            ));
            vec![format!("isu {id} ditandai selesai")]
        }
        Ok(false) => vec![format!("isu {id} tidak ditemukan")],
        Err(err) => vec![format!("gagal menyelesaikan isu: {err}")],
    }
}

/// Jalankan satu prompt di worktree git terisolasi.
async fn task_worktree(boot: &Boot, arg: &str) -> Vec<String> {
    let (keep, prompt) = match arg.strip_prefix("keep ") {
        Some(sisa) if !sisa.trim().is_empty() => (true, sisa.trim().to_string()),
        Some(_) => return vec!["pakai: /task-worktree [keep] <prompt>".to_string()],
        None if !arg.trim().is_empty() => (false, arg.trim().to_string()),
        None => return vec!["pakai: /task-worktree [keep] <prompt>".to_string()],
    };
    let id = crate::task_worktree::id_baru();
    let target = crate::task_worktree::worktree_path(&id);
    let root = boot.cwd.clone();
    let target_clone = target.clone();
    let dibuat =
        match tokio::task::spawn_blocking(move || crate::task_worktree::buat(&root, &target_clone))
            .await
        {
            Ok(Ok(())) => true,
            Ok(Err(err)) => return vec![format!("gagal membuat worktree: {err}")],
            Err(err) => return vec![format!("gagal membuat worktree: {err}")],
        };
    if !dibuat {
        return vec!["gagal membuat worktree".to_string()];
    }
    let exe = match std::env::current_exe() {
        Ok(v) => v,
        Err(err) => return vec![format!("gagal mencari binary maicli: {err}")],
    };
    let keluaran = tokio::process::Command::new(exe)
        .args(["--mode", "text", "--cwd"])
        .arg(&target)
        .args(["--prompt", &prompt])
        .output()
        .await;
    let (sukses, teks) = match keluaran {
        Ok(out) => (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        ),
        Err(err) => (false, err.to_string()),
    };
    let sukses = crate::task_worktree::sukses(sukses, &teks);
    let nilai = crate::task_worktree::hasil(&id, &target.display().to_string(), sukses, &teks);
    if !crate::task_worktree::valid(&nilai) {
        return vec!["hasil task tidak memenuhi skema".to_string()];
    }
    if !keep {
        let root = boot.cwd.clone();
        let target_clone = target.clone();
        let _ =
            tokio::task::spawn_blocking(move || crate::task_worktree::hapus(&root, &target_clone))
                .await;
    }
    vec![serde_json::to_string_pretty(&nilai).unwrap_or_default()]
}

/// Nama skill aman untuk path (kebab-case sederhana).
pub fn nama_skill_valid(nama: &str) -> bool {
    !nama.is_empty()
        && nama.len() <= 64
        && nama
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !nama.starts_with('-')
        && !nama.ends_with('-')
}

/// Path SKILL.md untuk satu skill.
pub fn skill_path(root: &std::path::Path, nama: &str) -> std::path::PathBuf {
    root.join("skills").join(nama).join("SKILL.md")
}

/// Buat skill baru dengan deskripsi.
fn skill_baru(root: &std::path::Path, arg: &str) -> Vec<String> {
    let (nama, deskripsi) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let deskripsi = deskripsi.trim();
    if !nama_skill_valid(nama) || deskripsi.is_empty() {
        return vec!["pakai: /skill-new <nama-kebab> <deskripsi>".to_string()];
    }
    let path = skill_path(root, nama);
    if path.exists() {
        return vec![format!("skill '{nama}' sudah ada")];
    }
    if let Some(induk) = path.parent()
        && std::fs::create_dir_all(induk).is_err()
    {
        return vec!["gagal membuat direktori skill".to_string()];
    }
    let isi = format!("description: {deskripsi}\n\n# {nama}\n\nTulis instruksi skill di sini.\n");
    match std::fs::write(&path, isi) {
        Ok(()) => vec![format!("skill '{nama}' dibuat: {}", path.display())],
        Err(err) => vec![format!("gagal menulis skill: {err}")],
    }
}

/// Hapus skill terkelola (perintah eksplisit pengguna).
fn skill_hapus(root: &std::path::Path, arg: &str) -> Vec<String> {
    let nama = arg.trim();
    if !nama_skill_valid(nama) {
        return vec!["pakai: /skill-rm <nama-kebab>".to_string()];
    }
    let path = skill_path(root, nama);
    let Some(induk) = path.parent() else {
        return vec!["path skill tidak valid".to_string()];
    };
    match std::fs::remove_dir_all(induk) {
        Ok(()) => vec![format!("skill '{nama}' dihapus")],
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            vec![format!("skill '{nama}' tidak ditemukan")]
        }
        Err(err) => vec![format!("gagal menghapus skill: {err}")],
    }
}

/// Daftar skill di workspace.
fn skill_daftar(root: &std::path::Path) -> Vec<String> {
    let mut hasil: Vec<String> = std::fs::read_dir(root.join("skills"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    hasil.sort();
    if hasil.is_empty() {
        vec!["belum ada skill di <cwd>/skills/".to_string()]
    } else {
        hasil.into_iter().map(|n| format!("- {n}")).collect()
    }
}

/// Jalankan perintah git baca-only di cwd.
fn git_baca(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let keluaran = std::process::Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if !keluaran.status.success() {
        return Err(String::from_utf8_lossy(&keluaran.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&keluaran.stdout).into_owned())
}

/// Ringkas status working tree menjadi satu baris.
pub fn ringkas_diagnose(status: &str, stat: &str) -> String {
    let berubah = status.lines().filter(|l| !l.trim().is_empty()).count();
    let file_stat = stat.lines().filter(|l| l.contains('|')).count();
    format!("{berubah} berkas berubah · {file_stat} baris statistik diff")
}

/// Kondisi workspace + sesi saat ini.
fn diagnose(boot: &Boot, session: &AgentSession) -> Vec<String> {
    let status = match git_baca(&boot.cwd, &["status", "--short"]) {
        Ok(v) => v,
        Err(err) => return vec![format!("diagnose gagal: {err}")],
    };
    let stat = git_baca(&boot.cwd, &["diff", "--stat", "HEAD"]).unwrap_or_default();
    let mut baris = vec![ringkas_diagnose(&status, &stat)];
    if !status.trim().is_empty() {
        baris.push(status.trim().to_string());
    }
    baris.push(format_budget(
        session.usage(),
        boot.limits.context_window_tokens,
    ));
    baris
}

/// Bandingkan working tree dengan ref git (default HEAD).
fn compare(boot: &Boot, arg: &str) -> Vec<String> {
    let target = if arg.trim().is_empty() {
        "HEAD"
    } else {
        arg.trim()
    };
    match git_baca(&boot.cwd, &["diff", "--stat", target]) {
        Ok(v) if v.trim().is_empty() => vec![format!("tidak ada perubahan terhadap {target}")],
        Ok(v) => vec![format!(
            "diff terhadap {target}:
{}",
            v.trim_end()
        )],
        Err(err) => vec![format!("compare gagal: {err}")],
    }
}

/// Pasang aturan TTSR/watchdog ke konteks.
fn watchdog(session: &mut AgentSession, arg: &str) -> Vec<String> {
    let aturan = crate::watchdog::parse_aturan(arg);
    if aturan.is_empty() {
        return vec!["pakai: /watchdog <pola1,pola2>".to_string()];
    }
    let contoh = aturan.join(" ");
    let status = match crate::watchdog::perlu_stop(&contoh, &aturan) {
        Some(p) => format!(" (pola aktif: {p})"),
        None => String::new(),
    };
    session.push_message(Message::system(crate::watchdog::blok(&aturan)));
    vec![format!("{} aturan TTSR dipasang{status}", aturan.len())]
}

/// Antrekan prewalk read-only.
fn prewalk(session: &mut AgentSession, arg: &str) -> Vec<String> {
    let goal = crate::goal_loop::muat();
    session.follow_up(crate::prewalk::prompt(&goal, arg));
    vec!["prewalk read-only diantrekan".to_string()]
}

/// Antrekan beberapa putaran riset read-only.
fn research(session: &mut AgentSession, arg: &str) -> Vec<String> {
    let (pertanyaan, angka) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let pertanyaan = pertanyaan.trim();
    if pertanyaan.is_empty() {
        return vec!["pakai: /research <pertanyaan> [putaran 2..5]".to_string()];
    }
    let n = match crate::research::putaran(angka.trim()) {
        Ok(n) => n,
        Err(err) => return vec![err],
    };
    for i in 1..=n {
        session.follow_up(crate::research::prompt_putaran(pertanyaan, i));
    }
    vec![format!("{n} putaran riset diantrekan (read-only)")]
}

/// Simpan kontrol think/yield sebagai pesan sistem.
fn thought(session: &mut AgentSession, kind: &str, arg: &str) -> Vec<String> {
    let teks = format!("{kind}: {arg}");
    let Some(t) = crate::thought::parse(&teks) else {
        return vec![format!("pakai: /{kind} <teks>")];
    };
    session.push_message(Message::system(crate::thought::blok(&t)));
    vec![format!("kontrol {} dicatat", t.kind)]
}

/// Kelola goal persisten.
fn goal(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let guided = arg.starts_with("guided ");
    let teks = arg.trim().trim_start_matches("guided ").trim();
    if teks.is_empty() {
        let isi = crate::goal_loop::muat();
        return vec![if isi.trim().is_empty() {
            "belum ada goal; pakai /goal <teks>".to_string()
        } else {
            isi
        }];
    }
    if teks == "clear" {
        return match crate::goal_loop::bersihkan() {
            Ok(()) => {
                session.set_system_prompt(base_system_prompt(boot, true));
                vec!["goal dihapus".to_string()]
            }
            Err(err) => vec![format!("gagal menghapus goal: {err}")],
        };
    }
    match crate::goal_loop::tulis(teks) {
        Ok(()) => {
            session.set_system_prompt(format!(
                "{}{}",
                base_system_prompt(boot, true),
                crate::goal_loop::blok(teks)
            ));
            if guided {
                session.follow_up(crate::thought::guided_prompt(teks));
                vec!["goal disimpan; langkah pertama diantrekan".to_string()]
            } else {
                vec!["goal disimpan".to_string()]
            }
        }
        Err(err) => vec![format!("gagal menyimpan goal: {err}")],
    }
}

/// Antrekan loop follow-up.
fn loop_goal(session: &mut AgentSession, arg: &str) -> Vec<String> {
    match crate::goal_loop::parse_loop(arg) {
        Ok((n, prompt)) => {
            for _ in 0..n {
                session.follow_up(prompt.clone());
            }
            vec![format!("{n} langkah loop diantrekan")]
        }
        Err(err) => vec![err],
    }
}

/// Satu langkah ratchet.
fn ratchet(session: &mut AgentSession, arg: &str) -> Vec<String> {
    let goal = crate::goal_loop::muat();
    match crate::goal_loop::parse_ratchet(arg, &goal) {
        Ok(prompt) => {
            session.follow_up(prompt);
            vec!["langkah ratchet diantrekan".to_string()]
        }
        Err(err) => vec![err],
    }
}

/// Daftar perubahan staging.
fn staging_daftar() -> Vec<String> {
    let semua = crate::staging::daftar(&crate::config_root());
    if semua.is_empty() {
        return vec!["tidak ada staging".to_string()];
    }
    semua
        .iter()
        .map(|s| format!("{} → {} ({} byte)", s.id, s.target, s.content.len()))
        .collect()
}

/// Terima satu staging.
fn staging_accept(boot: &Boot, arg: &str) -> Vec<String> {
    let id = arg.trim();
    if id.is_empty() {
        return vec!["pakai: /accept <id>".to_string()];
    }
    match crate::staging::terima(&crate::config_root(), &boot.cwd, id) {
        Ok(path) => vec![format!("staging diterima: {}", path.display())],
        Err(err) => vec![format!("accept gagal: {err}")],
    }
}

/// Kelola override system prompt pengguna.
fn system_prompt(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let teks = arg.trim().trim_start_matches("guided ").trim();
    if teks.is_empty() {
        let isi = crate::system_prompt::muat();
        return vec![if isi.trim().is_empty() {
            "belum ada override; pakai /system-prompt <teks>".to_string()
        } else {
            isi
        }];
    }
    if teks == "clear" {
        return match crate::system_prompt::bersihkan() {
            Ok(()) => {
                session.set_system_prompt(base_system_prompt(boot, true));
                vec!["override system prompt dihapus".to_string()]
            }
            Err(err) => vec![format!("gagal menghapus override: {err}")],
        };
    }
    match crate::system_prompt::tulis(teks) {
        Ok(()) => {
            session.set_system_prompt(format!(
                "{}{}",
                base_system_prompt(boot, true),
                crate::system_prompt::blok(teks)
            ));
            vec!["override system prompt disimpan".to_string()]
        }
        Err(err) => vec![format!("gagal menyimpan override: {err}")],
    }
}

/// Kelola rencana kerja: tampil, tulis, tambah, hapus.
fn plan(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let arg = arg.trim();
    if arg.is_empty() {
        let isi = crate::plan_mode::muat();
        return vec![if isi.trim().is_empty() {
            "belum ada rencana; pakai /plan <teks>".to_string()
        } else {
            isi
        }];
    }
    if arg == "clear" {
        return match crate::plan_mode::bersihkan() {
            Ok(()) => {
                session.set_system_prompt(base_system_prompt(boot, true));
                vec!["rencana dihapus".to_string()]
            }
            Err(err) => vec![format!("gagal menghapus rencana: {err}")],
        };
    }
    let (teks, tambah) = match arg.strip_prefix("append ") {
        Some(sisa) if !sisa.trim().is_empty() => (sisa.trim(), true),
        Some(_) => return vec!["pakai: /plan append <teks>".to_string()],
        None => (arg, false),
    };
    let hasil = if tambah {
        crate::plan_mode::tambah(teks)
    } else {
        crate::plan_mode::tulis(teks)
    };
    match hasil {
        Ok(()) => {
            let isi = crate::plan_mode::muat();
            session.set_system_prompt(format!(
                "{}\n\n## Rencana kerja aktif (/plan)\n\n{}",
                base_system_prompt(boot, true),
                isi.trim()
            ));
            vec!["rencana disimpan dan dipasang sebagai system prompt".to_string()]
        }
        Err(err) => vec![format!("gagal menyimpan rencana: {err}")],
    }
}

/// Tampilkan rencana untuk ditinjau.
fn plan_review() -> Vec<String> {
    let isi = crate::plan_mode::muat();
    if isi.trim().is_empty() {
        vec!["belum ada rencana untuk ditinjau".to_string()]
    } else {
        vec![format!("Rencana aktif:\n{}", isi.trim())]
    }
}

/// Tampilkan id instalasi lokal.
fn install_id() -> Vec<String> {
    match crate::install_id::dapatkan() {
        Ok(id) => vec![format!("install-id: {id} (lokal, tidak dikirim)")],
        Err(err) => vec![format!("gagal membaca install-id: {err}")],
    }
}

/// Direktori prompt files (per workspace).
pub fn prompt_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("prompts")
}

/// Daftar nama prompt `.md` tanpa ekstensi, terurut.
pub fn daftar_prompt(root: &std::path::Path) -> Vec<String> {
    let mut hasil: Vec<String> = std::fs::read_dir(prompt_dir(root))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_string)
        })
        .collect();
    hasil.sort();
    hasil
}

/// Kelola prompt files: tanpa argumen = daftar, `use <nama>` = pasang, `clear` = reset.
fn prompts(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let nama = arg.trim();
    if nama.is_empty() || nama == "list" {
        let semua = daftar_prompt(&boot.cwd);
        if semua.is_empty() {
            return vec!["belum ada prompt file di <cwd>/prompts/*.md".to_string()];
        }
        return semua
            .into_iter()
            .map(|n| format!("- {n} (/prompts use {n})"))
            .collect();
    }
    if nama == "clear" {
        session.set_system_prompt(base_system_prompt(boot, true));
        return vec!["system prompt dikembalikan ke default".to_string()];
    }
    let Some(nama) = nama
        .strip_prefix("use ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return vec!["pakai: /prompts [list|use <nama>|clear]".to_string()];
    };
    let path = prompt_dir(&boot.cwd).join(format!("{nama}.md"));
    let Ok(isi) = std::fs::read_to_string(&path) else {
        return vec![format!("prompt '{nama}' tidak ditemukan")];
    };
    let dasar = base_system_prompt(boot, true);
    session.set_system_prompt(format!("{dasar}\n\n## Prompt pengguna ({nama})\n{isi}"));
    vec![format!("prompt '{nama}' dipasang sebagai system prompt")]
}

/// Tambah notifikasi pengguna.
fn notifikasi_tambah(teks: &str) -> Vec<String> {
    match crate::context_notes::notification_tambah(teks) {
        Ok(n) => vec![format!("notifikasi disimpan ({n} total)")],
        Err(err) => vec![format!("gagal menyimpan notifikasi: {err}")],
    }
}

/// Daftar notifikasi pengguna.
fn notifikasi_daftar() -> Vec<String> {
    let semua = crate::context_notes::notification_muat();
    if semua.is_empty() {
        return vec!["belum ada notifikasi".to_string()];
    }
    semua
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}. {}", i + 1, n.text))
        .collect()
}

/// Hapus semua notifikasi.
fn notifikasi_bersihkan() -> Vec<String> {
    match crate::context_notes::notification_bersihkan() {
        Ok(()) => vec!["notifikasi dihapus".to_string()],
        Err(err) => vec![format!("gagal menghapus notifikasi: {err}")],
    }
}

/// Format ringkasan anggaran konteks (dipakai `/budget`).
pub fn format_budget(usage: tui_wire::Usage, window: u64) -> String {
    let pakai = usage.context_tokens();
    let persen = match pakai.saturating_mul(100).checked_div(window) {
        Some(nilai) => nilai.min(100),
        None => 0,
    };
    format!(
        "input: {} · output: {} · cache: {} · konteks: {}/{} ({}%) · sisa: {}",
        usage.input_tokens,
        usage.output_tokens,
        usage.cached_tokens,
        pakai,
        window,
        persen,
        window.saturating_sub(pakai)
    )
}

/// Ringkasan pemakaian token sesi.
fn budget(session: &AgentSession, boot: &Boot) -> Vec<String> {
    vec![format_budget(
        session.usage(),
        boot.limits.context_window_tokens,
    )]
}

/// Parse variabel editor (`VISUAL`/`EDITOR`) menjadi program + argumen.
pub fn editor_dari_env(nilai: &str) -> Option<(String, Vec<String>)> {
    let mut bagian = nilai.split_whitespace();
    let program = bagian.next()?.to_string();
    if program.is_empty() {
        return None;
    }
    Some((program, bagian.map(str::to_string).collect()))
}

/// Buka berkas di editor eksternal; tanpa argumen memakai `<data>/prompt.md`.
async fn edit_eksternal(boot: &Boot, arg: &str) -> Vec<String> {
    let target = if arg.trim().is_empty() {
        let path = crate::config_root().join("prompt.md");
        if !path.exists()
            && let Some(induk) = path.parent()
            && std::fs::create_dir_all(induk).is_ok()
        {
            let _ = std::fs::write(&path, "");
        }
        path
    } else {
        let relatif = std::path::PathBuf::from(arg.trim());
        if relatif.is_absolute() {
            relatif
        } else {
            boot.cwd.join(relatif)
        }
    };
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .and_then(|v| editor_dari_env(&v));
    let (program, args) = editor.unwrap_or_else(|| {
        if cfg!(windows) {
            ("notepad".to_string(), Vec::new())
        } else {
            ("vi".to_string(), Vec::new())
        }
    });
    match tokio::process::Command::new(&program)
        .args(&args)
        .arg(&target)
        .status()
        .await
    {
        Ok(status) if status.success() => vec![format!("editor selesai: {}", target.display())],
        Ok(status) => vec![format!("editor keluar dengan {status}")],
        Err(err) => vec![format!("gagal menjalankan '{program}': {err}")],
    }
}

/// Tambah root workspace untuk pencarian `glob`/`grep`.
fn tambah_dir(
    boot: &Boot,
    session: &mut AgentSession,
    state: &mut FeatureState,
    arg: &str,
) -> Vec<String> {
    let calon = std::path::PathBuf::from(arg.trim());
    if calon.as_os_str().is_empty() {
        return vec!["pakai: /add-dir <direktori>".to_string()];
    }
    let penuh = if calon.is_absolute() {
        calon
    } else {
        boot.cwd.join(calon)
    };
    let Ok(resmi) = std::fs::canonicalize(&penuh) else {
        return vec![format!("direktori tidak ditemukan: {}", penuh.display())];
    };
    if !resmi.is_dir() {
        return vec![format!("bukan direktori: {}", resmi.display())];
    }
    if resmi == boot.cwd || state.extra_dirs.contains(&resmi) {
        return vec![format!("direktori sudah terdaftar: {}", resmi.display())];
    }
    state.extra_dirs.push(resmi.clone());
    let mut roots = vec![boot.cwd.clone()];
    roots.extend(state.extra_dirs.iter().cloned());
    session.set_roots(roots);
    vec![format!("root ditambahkan: {}", resmi.display())]
}

/// Tampilkan atau hapus root tambahan.
fn daftar_dir(
    boot: &Boot,
    session: &mut AgentSession,
    state: &mut FeatureState,
    arg: &str,
) -> Vec<String> {
    if arg == "clear" {
        state.extra_dirs.clear();
        session.set_roots(vec![boot.cwd.clone()]);
        return vec!["root tambahan dihapus".to_string()];
    }
    let mut baris = vec![format!("0. {} (cwd)", boot.cwd.display())];
    baris.extend(
        state
            .extra_dirs
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}. {}", i + 1, p.display())),
    );
    baris
}

/// Ambil diff git ringkas untuk reviewer (tanpa mengubah working tree).
fn diff_untuk_review(cwd: &std::path::Path) -> Result<String, String> {
    let keluaran = std::process::Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(["diff", "--stat", "HEAD"])
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    if !keluaran.status.success() {
        return Err("bukan repositori git atau git diff gagal".to_string());
    }
    let patch = std::process::Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(["diff", "--unified=3", "HEAD"])
        .output()
        .map_err(|e| format!("git tidak tersedia: {e}"))?;
    let mut gabung = format!(
        "{}\n{}",
        String::from_utf8_lossy(&keluaran.stdout),
        String::from_utf8_lossy(&patch.stdout)
    );
    if gabung.len() > 24_000 {
        gabung.truncate(24_000);
        gabung.push_str("\n[…diff dipotong]");
    }
    Ok(gabung)
}

/// Mintalah review model kedua terhadap diff kerja.
async fn review_diff(boot: &Boot, state: &FeatureState, simpan: bool) -> Vec<String> {
    let diff = match diff_untuk_review(&boot.cwd) {
        Ok(d) if d.trim().is_empty() => {
            return vec!["tidak ada perubahan git untuk direview".to_string()];
        }
        Ok(d) => d,
        Err(err) => return vec![format!("review gagal: {err}")],
    };
    let model = if state.advisor_model.is_empty() {
        boot.model.clone()
    } else {
        state.advisor_model.clone()
    };
    let request = tui_provider::ChatRequest {
        model,
        messages: vec![
            Message::system(
                "Kamu reviewer kode teknis. Beri verdict ringkas dengan awalan P0/P1/P2/P3                  tiap poin, fokus pada bug, keamanan, dan regresi. Jangan mengulang diff.",
            ),
            Message::user(format!("Review diff berikut:\n\n{diff}")),
        ],
        tools: Vec::new(),
        max_tokens: Some(900),
        reasoning_effort: None,
        service_tier: None,
    };
    let receiver = if let Some((base, env)) = &state.advisor_provider {
        let key = std::env::var(env).ok().filter(|v| !v.trim().is_empty());
        match tui_provider::OpenAiCompatClient::new(base.clone(), key) {
            Ok(client) => tui_provider::Provider::stream_chat(&client, request).await,
            Err(err) => return vec![format!("review gagal: {}", err.message)],
        }
    } else {
        tui_provider::Provider::stream_chat(&boot.client, request).await
    };
    let mut receiver = match receiver {
        Ok(r) => r,
        Err(err) => return vec![format!("review gagal: {}", err.message)],
    };
    let mut teks = String::new();
    while let Some(event) = receiver.recv().await {
        match event {
            tui_wire::ProviderEvent::Done { message } => teks = message.joined_text(),
            tui_wire::ProviderEvent::Error { message, .. } => {
                return vec![format!("review gagal: {message}")];
            }
            _ => {}
        }
    }
    if teks.trim().is_empty() {
        return vec!["review: model tidak mengembalikan catatan".to_string()];
    }
    if simpan {
        match crate::context_notes::annotation_tambah(teks.trim()) {
            Ok(n) => vec![format!("review disimpan sebagai anotasi ({n} total)"), teks],
            Err(err) => vec![format!("review selesai tetapi gagal disimpan: {err}"), teks],
        }
    } else {
        vec![teks]
    }
}

/// Tambah anotasi manual.
fn annotate_tambah(teks: &str) -> Vec<String> {
    match crate::context_notes::annotation_tambah(teks) {
        Ok(n) => vec![format!("anotasi disimpan ({n} total)")],
        Err(err) => vec![format!("gagal menyimpan anotasi: {err}")],
    }
}

/// Daftar anotasi review.
fn annotate_daftar() -> Vec<String> {
    let semua = crate::context_notes::annotation_muat();
    if semua.is_empty() {
        return vec!["belum ada anotasi review".to_string()];
    }
    semua
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}. {}", i + 1, n.text))
        .collect()
}

/// Hapus semua anotasi review.
fn annotate_bersihkan() -> Vec<String> {
    match crate::context_notes::annotation_bersihkan() {
        Ok(()) => vec!["anotasi review dihapus".to_string()],
        Err(err) => vec![format!("gagal menghapus anotasi: {err}")],
    }
}

/// Daftar checkpoint yang tersimpan.
fn checkpoint_daftar() -> Vec<String> {
    let semua = crate::checkpoint::daftar();
    if semua.is_empty() {
        return vec!["belum ada checkpoint".to_string()];
    }
    semua
        .iter()
        .map(|c| format!("{} · {} · {} pesan", c.id, c.label, c.messages))
        .collect()
}

/// Simpan patch git + jumlah pesan saat ini.
async fn checkpoint_buat(
    cwd: &std::path::Path,
    session: &AgentSession,
    label: &str,
) -> Vec<String> {
    let jumlah = session.messages().len();
    let path = cwd.to_path_buf();
    let hasil = tokio::task::spawn_blocking(move || crate::checkpoint::simpan_patch(&path)).await;
    let Ok(Ok((id, patch))) = hasil else {
        return vec![format!("checkpoint gagal: {hasil:?}")];
    };
    match crate::checkpoint::tambah(id.clone(), label, jumlah, patch) {
        Ok(()) => vec![format!("checkpoint {id} disimpan ({jumlah} pesan)")],
        Err(err) => vec![format!("checkpoint gagal dicatat: {err}")],
    }
}

/// Kembalikan patch git dan potong transkrip ke checkpoint.
async fn rewind_ke(
    cwd: &std::path::Path,
    session: &mut AgentSession,
    store: &mut Option<SessionStore>,
    arg: &str,
) -> Vec<String> {
    let Some(cp) = crate::checkpoint::cari(arg) else {
        return vec!["checkpoint tidak ditemukan; pakai /checkpoint list".to_string()];
    };
    let path = cwd.to_path_buf();
    let target = cp.clone();
    let hasil =
        tokio::task::spawn_blocking(move || crate::checkpoint::revert(&path, &target)).await;
    match hasil {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return vec![format!("rewind git gagal: {err}")],
        Err(err) => return vec![format!("rewind gagal: {err}")],
    }
    let dipertahankan = match store.as_mut() {
        Some(s) => match s.rewind_messages(cp.messages) {
            Ok(n) => n,
            Err(err) => return vec![format!("rewind sesi gagal: {}", err.message)],
        },
        None => cp.messages,
    };
    session.truncate_messages(dipertahankan);
    vec![format!(
        "rewind ke {} selesai ({dipertahankan} pesan)",
        cp.id
    )]
}

/// Tambah catatan konteks persisten.
fn context_note_tambah(teks: &str) -> Vec<String> {
    match crate::context_notes::tambah(teks) {
        Ok(n) => vec![format!("catatan konteks disimpan ({n} total)")],
        Err(err) => vec![format!("gagal menyimpan catatan: {err}")],
    }
}

/// Daftar catatan konteks persisten.
fn context_note_daftar() -> Vec<String> {
    let semua = crate::context_notes::muat();
    if semua.is_empty() {
        return vec!["belum ada catatan konteks".to_string()];
    }
    semua
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}. {}", i + 1, n.text))
        .collect()
}

/// Hapus seluruh catatan konteks.
fn context_note_bersihkan() -> Vec<String> {
    match crate::context_notes::bersihkan() {
        Ok(()) => vec!["catatan konteks dihapus".to_string()],
        Err(err) => vec![format!("gagal menghapus catatan: {err}")],
    }
}

/// Ekspor transkrip teranonimkan (path disamarkan, kunci/token diredaksi).
async fn export_anon(
    _boot: &Boot,
    session: &AgentSession,
    store: &mut Option<SessionStore>,
) -> Vec<String> {
    let teks = transcript_markdown(session, true);
    let path = crate::config_root().join("export-anon.md");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&path, teks) {
        Ok(()) => {
            if let Some(store) = store.as_mut() {
                let _ = store.append(&SessionEntry::title_change("ekspor anonim"));
            }
            vec![format!("transkrip anonim ditulis: {}", path.display())]
        }
        Err(err) => vec![format!("export anon gagal: {err}")],
    }
}

/// CRC-32 IEEE (polinom 0xEDB88320) untuk ZIP *stored* tanpa dependensi ekstra.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Tulis satu entry ZIP (metode *stored*: tanpa kompresi, tanpa dependensi).
fn tulis_zip_entry(
    writer: &mut (impl Write + std::io::Seek),
    nama: &str,
    isi: &[u8],
    sentral: &mut Vec<u8>,
) -> Result<(), String> {
    let offset = writer.stream_position().map_err(|e| e.to_string())?;
    let nama = nama.as_bytes();
    let crc = crc32(isi);
    let mut lokal = Vec::with_capacity(30 + nama.len());
    lokal.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    lokal.extend_from_slice(&20u16.to_le_bytes()); // versi
    lokal.extend_from_slice(&0u16.to_le_bytes()); // flag
    lokal.extend_from_slice(&0u16.to_le_bytes()); // stored
    lokal.extend_from_slice(&0u16.to_le_bytes()); // jam
    lokal.extend_from_slice(&0u16.to_le_bytes()); // tanggal
    lokal.extend_from_slice(&crc.to_le_bytes());
    lokal.extend_from_slice(&(isi.len() as u32).to_le_bytes());
    lokal.extend_from_slice(&(isi.len() as u32).to_le_bytes());
    lokal.extend_from_slice(&(nama.len() as u16).to_le_bytes());
    lokal.extend_from_slice(&0u16.to_le_bytes()); // extra
    lokal.extend_from_slice(nama);
    writer.write_all(&lokal).map_err(|e| e.to_string())?;
    writer.write_all(isi).map_err(|e| e.to_string())?;
    sentral.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    sentral.extend_from_slice(&20u16.to_le_bytes()); // dibuat
    sentral.extend_from_slice(&20u16.to_le_bytes()); // needed
    sentral.extend_from_slice(&0u16.to_le_bytes());
    sentral.extend_from_slice(&0u16.to_le_bytes());
    sentral.extend_from_slice(&0u16.to_le_bytes());
    sentral.extend_from_slice(&0u16.to_le_bytes());
    sentral.extend_from_slice(&crc.to_le_bytes());
    sentral.extend_from_slice(&(isi.len() as u32).to_le_bytes());
    sentral.extend_from_slice(&(isi.len() as u32).to_le_bytes());
    sentral.extend_from_slice(&(nama.len() as u16).to_le_bytes());
    sentral.extend_from_slice(&0u16.to_le_bytes()); // extra
    sentral.extend_from_slice(&0u16.to_le_bytes()); // komentar
    sentral.extend_from_slice(&0u16.to_le_bytes()); // disk
    sentral.extend_from_slice(&0u16.to_le_bytes()); // atribut internal
    sentral.extend_from_slice(&0u32.to_le_bytes()); // atribut eksternal
    sentral.extend_from_slice(&(offset as u32).to_le_bytes());
    sentral.extend_from_slice(nama);
    Ok(())
}

/// Ekspor sesi ke arsip zip: transkrip + berkas sesi JSONL + metadata.
fn export_zip(boot: &Boot, session: &AgentSession, store: &Option<SessionStore>) -> Vec<String> {
    use std::io::Seek;

    let path = crate::config_root().join("maicli-export.zip");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let hasil = (|| -> Result<(), String> {
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut sentral = Vec::new();
        let transkrip = transcript_markdown(session, false);
        tulis_zip_entry(
            &mut file,
            "transcript.md",
            transkrip.as_bytes(),
            &mut sentral,
        )?;
        if let Some(store) = store.as_ref()
            && let Ok(isi) = std::fs::read(store.path())
        {
            tulis_zip_entry(&mut file, "session.jsonl", &isi, &mut sentral)?;
        }
        let meta = format!(
            "model: {}\nprovider: {}\nrequested_by: maicli {}\n",
            session.model(),
            boot.provider.0,
            env!("CARGO_PKG_VERSION")
        );
        tulis_zip_entry(&mut file, "metadata.txt", meta.as_bytes(), &mut sentral)?;
        let offset = file.stream_position().map_err(|e| e.to_string())?;
        file.write_all(&sentral).map_err(|e| e.to_string())?;
        let mut eocd = Vec::with_capacity(22);
        eocd.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        // Jumlah entry: 2 bila session.jsonl ada, selalu minimal transcript+meta.
        let jumlah: u16 = if store
            .as_ref()
            .is_some_and(|s| std::fs::metadata(s.path()).is_ok())
        {
            3
        } else {
            2
        };
        eocd.extend_from_slice(&jumlah.to_le_bytes());
        eocd.extend_from_slice(&jumlah.to_le_bytes());
        eocd.extend_from_slice(&(sentral.len() as u32).to_le_bytes());
        eocd.extend_from_slice(&(offset as u32).to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        file.write_all(&eocd).map_err(|e| e.to_string())?;
        Ok(())
    })();
    match hasil {
        Ok(()) => vec![format!("arsip zip ditulis: {}", path.display())],
        Err(err) => vec![format!("export zip gagal: {err}")],
    }
}

/// Redaksi pola kredensial umum dari teks ekspor.
fn redaksi(teks: &str) -> String {
    let mut hasil = String::with_capacity(teks.len());
    for token in teks.split_inclusive(char::is_whitespace) {
        let bersih = token.trim_end();
        let lower = bersih.to_ascii_lowercase();
        let sensitif = lower.contains("sk-")
            || lower.contains("api_key=")
            || lower.contains("authorization: bearer")
            || lower.contains("token=");
        if sensitif {
            hasil.push_str("[REDACTED]");
        } else {
            hasil.push_str(bersih);
        }
        if token.len() > bersih.len() {
            hasil.push(' ');
        }
    }
    redaksi_path(&hasil)
}

/// Samarkan nama pengguna pada path umum (`/home/<user>/`, `C:\Users\<user>\`).
fn redaksi_path(teks: &str) -> String {
    let mut out = String::with_capacity(teks.len());
    let bytes = teks.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let sisa = &teks[index..];
        let (prefix, pemisah) = if sisa.starts_with("/home/") {
            ("/home/", b'/')
        } else if sisa.starts_with("C:\\Users\\") {
            ("C:\\Users\\", b'\\')
        } else {
            let ch = sisa.chars().next().unwrap_or(' ');
            out.push(ch);
            index += ch.len_utf8();
            continue;
        };
        out.push_str(prefix);
        index += prefix.len();
        let mulai = index;
        while index < bytes.len() && bytes[index] != pemisah {
            index += 1;
        }
        if index > mulai {
            out.push_str("<user>");
        } else {
            out.push_str(&teks[mulai..index]);
        }
    }
    out
}

/// Render daftar todo menjadi Markdown.
fn todo_markdown(daftar: &[String]) -> String {
    let mut teks = String::from("# Todo\n\n");
    for item in daftar {
        let baris = if let Some(sisa) = item.strip_prefix("[selesai] ") {
            format!("- [x] {sisa}")
        } else if let Some(sisa) = item.strip_prefix("[mulai] ") {
            format!("- [>] {sisa}")
        } else if let Some(sisa) = item.strip_prefix("[dibuang] ") {
            format!("- [-] {sisa}")
        } else {
            format!("- [ ] {item}")
        };
        teks.push_str(&baris);
        teks.push('\n');
    }
    teks
}

/// Ambil daftar todo dari state.
fn todo_list(state: &FeatureState) -> Result<Vec<String>, String> {
    state
        .todo
        .as_ref()
        .ok_or_else(|| "mode ini tidak punya daftar todo".to_string())?
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "daftar todo terkunci".to_string())
}

/// Tulis todo sesi ke Markdown (`/todo export [berkas]`).
fn todo_export(state: &FeatureState, arg: &str) -> Vec<String> {
    let daftar = match todo_list(state) {
        Ok(daftar) => daftar,
        Err(pesan) => return vec![pesan],
    };
    let path = if arg.is_empty() {
        PathBuf::from("todo.md")
    } else {
        PathBuf::from(arg)
    };
    match std::fs::write(&path, todo_markdown(&daftar)) {
        Ok(()) => vec![format!("{} todo ditulis: {}", daftar.len(), path.display())],
        Err(err) => vec![format!("todo export gagal: {err}")],
    }
}

/// Impor todo dari Markdown (`/todo import <berkas>`).
fn todo_import(state: &mut FeatureState, arg: &str) -> Vec<String> {
    if arg.is_empty() {
        return vec!["pakai: /todo import <berkas>".to_string()];
    }
    let isi = match std::fs::read_to_string(arg) {
        Ok(isi) => isi,
        Err(err) => return vec![format!("todo import gagal: {err}")],
    };
    let mut daftar = Vec::new();
    for line in isi.lines() {
        let line = line.trim();
        if let Some(sisa) = line.strip_prefix("- [x] ") {
            daftar.push(format!("[selesai] {sisa}"));
        } else if let Some(sisa) = line
            .strip_prefix("- [>] ")
            .or_else(|| line.strip_prefix("- [~] "))
            .or_else(|| line.strip_prefix("- [/] "))
        {
            daftar.push(format!("[mulai] {sisa}"));
        } else if let Some(sisa) = line.strip_prefix("- [-] ") {
            daftar.push(format!("[dibuang] {sisa}"));
        } else if let Some(sisa) = line.strip_prefix("- [ ] ") {
            daftar.push(sisa.to_string());
        } else if let Some(sisa) = line.strip_prefix("- ") {
            daftar.push(sisa.to_string());
        }
    }
    if daftar.is_empty() {
        return vec![format!("tidak ada item todo yang dikenali di {arg}")];
    }
    let Some(arc) = state.todo.as_ref() else {
        return vec!["mode ini tidak punya daftar todo".to_string()];
    };
    match arc.lock() {
        Ok(mut guard) => {
            *guard = daftar.clone();
            vec![format!("{} todo diimpor dari {arg}", daftar.len())]
        }
        Err(_) => vec!["daftar todo terkunci".to_string()],
    }
}

/// Salin todo ke clipboard.
fn todo_copy(state: &FeatureState) -> Vec<String> {
    let daftar = match todo_list(state) {
        Ok(daftar) => daftar,
        Err(pesan) => return vec![pesan],
    };
    let teks = todo_markdown(&daftar);
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(teks)) {
        Ok(()) => vec![format!("{} todo disalin ke clipboard", daftar.len())],
        Err(err) => vec![format!("clipboard tidak tersedia: {err}")],
    }
}

/// Hapus sesi tersimpan berdasarkan id/prefix (`/delete <id>`).
async fn delete_session(boot: &Boot, arg: &str) -> Vec<String> {
    if arg.is_empty() {
        return vec!["pakai: /delete <id-sesi> (lihat /sessions)".to_string()];
    }
    let dir = boot.sessions_dir.clone();
    let wanted = arg.to_string();
    let hasil = tokio::task::spawn_blocking(move || {
        let summary = tui_session::resolve_target(&dir, &wanted)?;
        tui_session::remove(&summary.path)?;
        Ok::<_, tui_wire::MxError>(summary)
    })
    .await;
    match hasil {
        Ok(Ok(summary)) => vec![format!(
            "sesi {} dihapus ({})",
            summary.id,
            summary.path.display()
        )],
        Ok(Err(err)) => vec![format!("{}: {}", err.code, err.message)],
        Err(err) => vec![format!("delete gagal: {err}")],
    }
}

/// Pembersih konteks bertingkat: `keep` hasil tool + opsional buang thinking.
fn cleaner(session: &mut AgentSession, keep: usize, thinking: bool) -> Vec<String> {
    let pruned = session.apply_elide(keep);
    let removed = if thinking {
        session.apply_strip_thinking()
    } else {
        0
    };
    let mut lines = vec![format!(
        "konteks dipangkas: {pruned} hasil tool (keep {keep}), {removed} blok thinking"
    )];
    lines.push(format!(
        "perkiraan token sekarang: ~{}",
        session.estimated_tokens()
    ));
    lines
}

fn record(boot: &Boot, arg: &str) -> Vec<String> {
    match arg {
        "" | "status" => vec![format!(
            "record: {} → {}",
            if boot.recorder.is_enabled() {
                "on"
            } else {
                "off"
            },
            boot.recorder.path().display()
        )],
        "on" => {
            boot.recorder.set_enabled(true);
            vec![format!("record: on → {}", boot.recorder.path().display())]
        }
        "off" => {
            boot.recorder.set_enabled(false);
            vec![format!(
                "record: off (berkas terakhir: {})",
                boot.recorder.path().display()
            )]
        }
        lain => vec![format!(
            "argumen record '{lain}' tidak dikenal; pakai on, off, status"
        )],
    }
}

fn memory(boot: &Boot, arg: &str) -> Vec<String> {
    let Some(memory) = boot.memory.as_ref() else {
        return vec!["memori dimatikan (config [memory] enabled=false)".to_string()];
    };
    let (sub, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let rest = rest.trim();
    let limit = rest.parse::<usize>().ok().unwrap_or(10).clamp(1, 50);
    match sub {
        "" | "view" => match memory.recent(limit) {
            Ok(entries) if entries.is_empty() => vec!["memori masih kosong".to_string()],
            Ok(entries) => {
                let mut lines = vec![format!("memori ({} entry terbaru):", entries.len())];
                lines.extend(entries.iter().map(|entry| {
                    let tags = if entry.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", entry.tags.join(","))
                    };
                    format!("  {} {}{tags}", entry.id, entry.text)
                }));
                lines
            }
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        "add" => {
            if rest.is_empty() {
                return vec!["pakai: /memory add <teks>".to_string()];
            }
            match memory.add(rest, &[]) {
                Ok(entry) => vec![format!("memori disimpan: {}", entry.id)],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "search" => {
            if rest.is_empty() {
                return vec!["pakai: /memory search <kata kunci>".to_string()];
            }
            match memory.search(rest, 10) {
                Ok(hits) if hits.is_empty() => vec!["tidak ada memori yang cocok".to_string()],
                Ok(hits) => hits
                    .iter()
                    .map(|hit| format!("  {:.2}  {}  {}", hit.score, hit.entry.id, hit.entry.text))
                    .collect(),
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "forget" => {
            if rest.is_empty() {
                return vec!["pakai: /memory forget <id>".to_string()];
            }
            match memory.forget(rest) {
                Ok(true) => vec![format!("memori {rest} dihapus")],
                Ok(false) => vec![format!("id {rest} tidak ditemukan")],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "stats" => match memory.stats() {
            Ok(stats) => vec![format!(
                "memori: backend={} entries={} embedding={} dir={}",
                stats.backend,
                stats.entries,
                stats.embedding,
                stats.dir.display()
            )],
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        "backend" => {
            if !matches!(rest, "jsonl" | "markdown" | "sqlite") {
                return vec!["pakai: /memory backend jsonl|markdown|sqlite".to_string()];
            }
            config_set("memory.backend", rest, "backend memori")
        }
        "embedding" => {
            if !matches!(rest, "hash" | "none") {
                return vec!["pakai: /memory embedding hash|none".to_string()];
            }
            config_set("memory.embedding", rest, "embedding memori")
        }
        lain => vec![format!(
            "subcommand memory '{lain}' tidak dikenal; pakai view, add, search, forget, stats, backend, embedding"
        )],
    }
}

fn config_set(key: &str, value: &str, label: &str) -> Vec<String> {
    match tui_config::set_global_value(&global_config_path(), key, value) {
        Ok(()) => vec![format!(
            "{label}: {value} (config global; restart agar berlaku)"
        )],
        Err(err) => vec![format!("{}: {}", err.code, err.message)],
    }
}

/// Tambah server MCP HTTP ke config global (`/mcp add <id> <url>`).
async fn mcp_add(arg: &str) -> Vec<String> {
    let Some((id, url)) = arg.split_once(char::is_whitespace) else {
        return vec!["pakai: /mcp add <id> <url-http(s)>".to_string()];
    };
    let (id, url) = (id.trim(), url.trim());
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || id.is_empty()
    {
        return vec!["id server hanya alfanumerik/-/_".to_string()];
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return vec!["url server harus http(s)://".to_string()];
    }
    let path = global_config_path();
    let hasil = (|| {
        tui_config::set_global_value(&path, &format!("mcp.servers.{id}.url"), url)?;
        tui_config::set_global_value(&path, &format!("mcp.servers.{id}.transport"), "http")?;
        tui_config::set_global_value(&path, &format!("mcp.servers.{id}.enabled"), "true")
    })();
    match hasil {
        Ok(()) => vec![format!(
            "server MCP '{id}' ditambahkan ({url}); restart agar terhubung"
        )],
        Err(err) => vec![format!("{}: {}", err.code, err.message)],
    }
}

/// Uji satu server MCP dengan menghubungkannya sekarang (`/mcp test <id>`).
async fn mcp_test(boot: &Boot, server_id: &str) -> Vec<String> {
    let config = config_sekarang(boot);
    let Some(server) = config.mcp.servers.get(server_id) else {
        return vec![format!("server MCP '{server_id}' tidak ada di config")];
    };
    let mut satu = tui_config::McpConfig {
        timeout_secs: config.mcp.timeout_secs,
        ..tui_config::McpConfig::default()
    };
    satu.servers.insert(server_id.to_string(), server.clone());
    let hasil = crate::mcp_boot::connect_all(&satu, &crate::config_root(), &boot.cwd).await;
    let mut lines = hasil.notes;
    lines.push(format!("tool terdeteksi: {}", hasil.tools.len()));
    lines
}

/// Nyalakan/matikan server MCP di config global.
fn mcp_toggle(boot: &Boot, server_id: &str, enabled: bool) -> Vec<String> {
    let config = config_sekarang(boot);
    if !config.mcp.servers.contains_key(server_id) {
        return vec![format!("server MCP '{server_id}' tidak ada di config")];
    }
    let value = if enabled { "true" } else { "false" };
    config_set(
        &format!("mcp.servers.{server_id}.enabled"),
        value,
        &format!("server MCP '{server_id}'"),
    )
}

async fn mcp_reauth(boot: &Boot, server_id: &str) -> Vec<String> {
    let Some(server) = boot.config.mcp.servers.get(server_id) else {
        return vec![format!("server MCP '{server_id}' tidak ada di config")];
    };
    let Some(oauth) = &server.oauth else {
        return vec![format!(
            "server '{server_id}' tidak punya [mcp.servers.{server_id}.oauth]"
        )];
    };
    let Some(url) = server.url.as_deref() else {
        return vec![format!(
            "server '{server_id}' memakai stdio; OAuth hanya untuk server url"
        )];
    };
    let cfg = crate::mcp_boot::oauth_config(oauth, url);
    let path = tui_mcp::oauth::token_path(&crate::config_root(), server_id);
    match tui_mcp::oauth::reauth(&cfg, &path).await {
        Ok(_) => vec![
            format!(
                "OAuth '{server_id}' berhasil; token disimpan di {}",
                path.display()
            ),
            "restart maicli agar server MCP terhubung dengan token baru".to_string(),
        ],
        Err(err) => vec![format!("{}: {}", err.code, err.message)],
    }
}

async fn marketplace(boot: &Boot, arg: &str) -> Vec<String> {
    if !boot.config.marketplace.enabled {
        return vec!["marketplace dimatikan di config".to_string()];
    }
    let dir = crate::extensions_dir();
    let (sub, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let rest = rest.trim();
    match sub {
        "" | "list" => daftar_extension(&dir),
        "install" => {
            if rest.is_empty() {
                return vec!["pakai: /marketplace install <url-git>".to_string()];
            }
            match tui_tools::marketplace::install_git(
                rest,
                &dir,
                boot.config.marketplace.allow_insecure,
            )
            .await
            {
                Ok(ext) => vec![
                    format!("extension '{}' v{} terpasang", ext.name, ext.version),
                    "jalankan /plugins reload atau restart agar tool aktif".to_string(),
                ],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "remove" | "rm" => match tui_tools::marketplace::remove(rest, &dir) {
            Ok(path) => vec![format!("extension dihapus: {}", path.display())],
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        "update" => {
            if rest.is_empty() {
                let daftar = tui_tools::marketplace::list(&dir);
                if daftar.is_empty() {
                    return vec!["belum ada extension terpasang".to_string()];
                }
                let mut lines = Vec::new();
                for ext in daftar {
                    match tui_tools::marketplace::update(&ext.name, &dir).await {
                        Ok(ext) => lines.push(format!("{}: v{}", ext.name, ext.version)),
                        Err(err) => lines.push(format!("{}: {}", ext.name, err.message)),
                    }
                }
                return lines;
            }
            match tui_tools::marketplace::update(rest, &dir).await {
                Ok(ext) => vec![format!("{} diperbarui: v{}", ext.name, ext.version)],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "search" => {
            if boot.config.marketplace.index_url.trim().is_empty() {
                return vec![
                    "marketplace.index_url belum diisi; set ke URL index.json".to_string(),
                ];
            }
            let client = match reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
            {
                Ok(client) => client,
                Err(err) => return vec![format!("klien HTTP gagal: {err}")],
            };
            let response = match client.get(&boot.config.marketplace.index_url).send().await {
                Ok(response) => response,
                Err(err) => return vec![format!("indeks tidak bisa diambil: {err}")],
            };
            if !response.status().is_success() {
                return vec![format!("indeks mengembalikan HTTP {}", response.status())];
            }
            let body = match response.bytes().await {
                Ok(body) => body,
                Err(err) => return vec![format!("indeks tidak bisa dibaca: {err}")],
            };
            if body.len() > 1_048_576 {
                return vec!["indeks melebihi 1 MiB; ditolak".to_string()];
            }
            let index: tui_tools::marketplace::Index = match serde_json::from_slice(&body) {
                Ok(index) => index,
                Err(err) => return vec![format!("format indeks tidak valid: {err}")],
            };
            let kata = rest.to_lowercase();
            let mut lines = Vec::new();
            for entry in index.extensions {
                let cocok = kata.is_empty()
                    || entry.name.to_lowercase().contains(&kata)
                    || entry.description.to_lowercase().contains(&kata);
                if cocok {
                    lines.push(format!(
                        "{} v{} — {} ({})",
                        entry.name, entry.version, entry.description, entry.url
                    ));
                }
            }
            if lines.is_empty() {
                vec!["tidak ada extension yang cocok".to_string()]
            } else {
                lines
            }
        }
        lain => vec![format!(
            "subcommand marketplace '{lain}' tidak dikenal; pakai list, search, install, remove, update"
        )],
    }
}

fn daftar_extension(dir: &Path) -> Vec<String> {
    let daftar = tui_tools::marketplace::list(dir);
    if daftar.is_empty() {
        return vec![format!("belum ada extension di {}", dir.display())];
    }
    let mut lines = vec![format!("extension terpasang: {}", daftar.len())];
    lines.extend(
        daftar
            .iter()
            .map(|ext| format!("  {} v{} — {}", ext.name, ext.version, ext.path.display())),
    );
    lines
}

async fn plugins(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    if !boot.config.extensions.enabled {
        return vec!["extension dimatikan di config [extensions]".to_string()];
    }
    let dir = crate::extensions_dir();
    match arg {
        "" | "list" => daftar_extension(&dir),
        "reload" => {
            let mut catatan = Vec::new();
            let tools = tui_tools::extensions::muat_extensions(&dir, &mut catatan);
            if tools.is_empty() && !catatan.is_empty() {
                let mut lines = vec!["tidak ada tool extension yang dimuat".to_string()];
                lines.extend(catatan);
                return lines;
            }
            let jumlah = tools.len();
            for tool in tools {
                session.registry_mut().replace_named(tool);
            }
            let mut lines = vec![format!("{jumlah} tool extension dimuat ulang")];
            lines.extend(catatan);
            lines.push("tool extension yang dihapus tetap aktif sampai restart".to_string());
            lines
        }
        lain => vec![format!(
            "subcommand plugins '{lain}' tidak dikenal; pakai list atau reload"
        )],
    }
}

async fn ssh(boot: &Boot, session: &mut AgentSession, arg: &str) -> Vec<String> {
    let config = config_sekarang(boot);
    let (sub, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let rest = rest.trim();
    match sub {
        "" | "list" => {
            if config.ssh.hosts.is_empty() {
                return vec![
                    "belum ada host SSH (pakai /ssh add <nama> <user@host[:port]>)".to_string(),
                ];
            }
            let mut lines = vec![format!("host SSH: {}", config.ssh.hosts.len())];
            for (name, host) in &config.ssh.hosts {
                lines.push(format!(
                    "  {name}: {} port {} timeout {}s",
                    host.target, host.port, host.timeout_secs
                ));
            }
            lines
        }
        "add" => {
            let mut bagian = rest.split_whitespace();
            let (Some(name), Some(target)) = (bagian.next(), bagian.next()) else {
                return vec!["pakai: /ssh add <nama> <user@host[:port]> [identity]".to_string()];
            };
            let identity = bagian.next().unwrap_or("").to_string();
            let (target, port) = pisah_port(target);
            let host = tui_config::SshHostConfig {
                target: target.to_string(),
                port,
                identity,
                timeout_secs: 60,
            };
            let path = global_config_path();
            match tui_config::set_global_ssh_host(&path, name, &host) {
                Ok(()) => {
                    let mut lines = vec![format!("host '{name}' disimpan")];
                    lines.extend(reload_ssh(boot, session));
                    lines
                }
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "remove" | "rm" => {
            if rest.is_empty() {
                return vec!["pakai: /ssh remove <nama>".to_string()];
            }
            let path = global_config_path();
            match tui_config::remove_global_value(&path, &format!("ssh.hosts.{rest}")) {
                Ok(true) => {
                    let mut lines = vec![format!("host '{rest}' dihapus")];
                    lines.extend(reload_ssh(boot, session));
                    lines
                }
                Ok(false) => vec![format!("host '{rest}' tidak ada")],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "test" => {
            let Some(host) = config.ssh.hosts.get(rest) else {
                return vec![format!("host '{rest}' tidak ada")];
            };
            let mut hosts = std::collections::BTreeMap::new();
            hosts.insert(
                rest.to_string(),
                tui_tools::ssh::SshHost {
                    target: host.target.clone(),
                    port: host.port,
                    identity: host.identity.clone(),
                    timeout_secs: host.timeout_secs,
                },
            );
            let tool = tui_tools::ssh::SshTool::new(hosts);
            let ctx = tui_tools::ToolContext::new(&boot.cwd);
            match tui_tools::Tool::execute(
                &tool,
                &serde_json::json!({"host": rest, "command": "true"}),
                &ctx,
            )
            .await
            {
                Ok(output) if output.is_error => vec![format!("gagal: {}", output.content)],
                Ok(_) => vec![format!("host '{rest}' terhubung")],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        lain => vec![format!(
            "subcommand ssh '{lain}' tidak dikenal; pakai list, add, remove, test"
        )],
    }
}

fn pisah_port(target: &str) -> (&str, u16) {
    if let Some((host, port)) = target.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
        && !host.is_empty()
    {
        return (host, port);
    }
    (target, 22)
}

/// Muat ulang tool SSH dari config setelah add/remove.
fn reload_ssh(boot: &Boot, session: &mut AgentSession) -> Vec<String> {
    let global = tui_config::default_global_path();
    let project = tui_config::default_project_path(&boot.cwd);
    let loaded = match tui_config::load(global.as_deref(), Some(&project)) {
        Ok(loaded) => loaded,
        Err(err) => return vec![format!("config gagal dimuat ulang: {}", err.message)],
    };
    let hosts: std::collections::BTreeMap<String, tui_tools::ssh::SshHost> = loaded
        .config
        .ssh
        .hosts
        .iter()
        .map(|(name, host)| {
            (
                name.clone(),
                tui_tools::ssh::SshHost {
                    target: host.target.clone(),
                    port: host.port,
                    identity: host.identity.clone(),
                    timeout_secs: host.timeout_secs,
                },
            )
        })
        .collect();
    if hosts.is_empty() {
        session.registry_mut().remove_named("ssh");
        return vec!["tool ssh dilepas (tidak ada host)".to_string()];
    }
    session
        .registry_mut()
        .replace_named(std::sync::Arc::new(tui_tools::ssh::SshTool::new(hosts)));
    vec!["tool ssh dimuat ulang dari config".to_string()]
}

async fn computer(boot: &Boot, arg: &str) -> Vec<String> {
    let (sub, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    if sub == "script" {
        return computer_script(boot, rest.trim()).await;
    }
    let rest = rest.trim();
    let action = if sub.is_empty() { "status" } else { sub };
    let mut args = serde_json::json!({ "action": action });
    match action {
        "status" => {}
        "screenshot" => {
            if !rest.is_empty() {
                args["path"] = serde_json::Value::String(rest.to_string());
            }
        }
        "type" => {
            if rest.is_empty() {
                return vec!["pakai: /computer type <teks>".to_string()];
            }
            args["text"] = serde_json::Value::String(rest.to_string());
        }
        "key" => {
            if rest.is_empty() {
                return vec!["pakai: /computer key <chord, mis. ctrl+c>".to_string()];
            }
            args["keys"] = serde_json::Value::String(rest.to_string());
        }
        "move" | "click" => {
            let mut angka: Vec<i64> = Vec::new();
            let mut tombol: Option<String> = None;
            for bagian in rest.split_whitespace() {
                match bagian.parse::<i64>() {
                    Ok(n) => angka.push(n),
                    Err(_) if action == "click" && tombol.is_none() => {
                        tombol = Some(bagian.to_string());
                    }
                    Err(_) => {
                        return vec![format!("argumen '{bagian}' bukan angka")];
                    }
                }
            }
            if action == "move" && angka.len() < 2 {
                return vec!["pakai: /computer move <x> <y>".to_string()];
            }
            if let Some(button) = tombol {
                args["button"] = serde_json::Value::String(button);
            }
            if let Some(x) = angka.first() {
                args["x"] = serde_json::json!(x);
            }
            if let Some(y) = angka.get(1) {
                args["y"] = serde_json::json!(y);
            }
        }
        "scroll" => {
            let dy: i64 = match rest.parse() {
                Ok(dy) => dy,
                Err(_) => return vec!["pakai: /computer scroll <langkah>".to_string()],
            };
            args["dy"] = serde_json::json!(dy);
        }
        lain => {
            return vec![format!(
                "subcommand computer '{lain}' tidak dikenal; pakai status, screenshot, move, click, scroll, type, key"
            )];
        }
    }
    let tool = tui_computer::ComputerTool;
    let ctx = tui_tools::ToolContext::new(&boot.cwd);
    match tui_tools::Tool::execute(&tool, &args, &ctx).await {
        Ok(output) if output.is_error => vec![format!("gagal: {}", output.content)],
        Ok(output) => vec![output.content],
        Err(err) => vec![format!("{}: {}", err.code, err.message)],
    }
}

/// Jalankan urutan langkah `/computer` dari array JSON (maks. 32 langkah).
///
/// Tiap langkah = objek `{"action":"click","x":10,"y":20,...}`. Eksekusi berhenti
/// pada langkah gagal supaya state GUI tidak melanjut ke aksi yang tidak valid.
async fn computer_script(boot: &Boot, arg: &str) -> Vec<String> {
    let Ok(steps) = serde_json::from_str::<Vec<serde_json::Value>>(arg) else {
        return vec!["pakai: /computer script [{\"action\":\"status\"}]".to_string()];
    };
    if steps.is_empty() || steps.len() > 32 {
        return vec!["script computer butuh 1..32 langkah".to_string()];
    }
    let tool = tui_computer::ComputerTool;
    let ctx = tui_tools::ToolContext::new(&boot.cwd);
    let mut hasil = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let Some(action) = step.get("action").and_then(serde_json::Value::as_str) else {
            return vec![format!("langkah {} tanpa action", i + 1)];
        };
        if !matches!(
            action,
            "status" | "screenshot" | "move" | "click" | "scroll" | "type" | "key"
        ) {
            return vec![format!(
                "langkah {}: action '{action}' tidak dikenal",
                i + 1
            )];
        }
        let mut args = serde_json::json!({ "action": action });
        for field in ["path", "text", "keys", "button", "x", "y", "dy"] {
            if let Some(value) = step.get(field) {
                args[field] = value.clone();
            }
        }
        match tui_tools::Tool::execute(&tool, &args, &ctx).await {
            Ok(output) if output.is_error => {
                hasil.push(format!("langkah {} gagal: {}", i + 1, output.content));
                return hasil;
            }
            Ok(output) => hasil.push(format!("langkah {}: {}", i + 1, output.content)),
            Err(err) => {
                hasil.push(format!(
                    "langkah {} error: {}: {}",
                    i + 1,
                    err.code,
                    err.message
                ));
                return hasil;
            }
        }
    }
    hasil
}

fn mental(boot: &Boot, arg: &str) -> Vec<String> {
    let Some(memory) = boot.memory.as_ref() else {
        return vec!["memori dimatikan; /mm butuh memory aktif".to_string()];
    };
    let (sub, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let rest = rest.trim();
    match sub {
        "" | "list" => match memory.list_models() {
            Ok(models) if models.is_empty() => vec!["belum ada mental model".to_string()],
            Ok(models) => {
                let mut lines = vec![format!("mental model: {}", models.len())];
                lines.extend(
                    models
                        .iter()
                        .map(|model| format!("  {}: {}", model.name, model.ringkas)),
                );
                lines
            }
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        "show" => match memory.read_model(rest) {
            Ok(model) => vec![format!("# {}\n{}", model.name, model.isi)],
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        "add" => {
            let Some((name, isi)) = rest.split_once(char::is_whitespace) else {
                return vec!["pakai: /mm add <nama> <isi>".to_string()];
            };
            match memory.write_model(name, isi) {
                Ok(model) => vec![format!("mental model '{}' disimpan", model.name)],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "refresh" => {
            if rest.is_empty() || rest == "all" || rest == "semua" {
                return match memory.refresh_all_models() {
                    Ok(0) => vec!["belum ada mental model untuk di-refresh".to_string()],
                    Ok(jumlah) => vec![format!("{jumlah} mental model di-refresh dari memori")],
                    Err(err) => vec![format!("{}: {}", err.code, err.message)],
                };
            }
            match memory.refresh_model(rest) {
                Ok(model) => vec![format!(
                    "mental model '{}' di-refresh dari memori",
                    model.name
                )],
                Err(err) => vec![format!("{}: {}", err.code, err.message)],
            }
        }
        "rm" | "remove" => match memory.remove_model(rest) {
            Ok(true) => vec![format!("mental model '{rest}' dihapus")],
            Ok(false) => vec![format!("mental model '{rest}' tidak ada")],
            Err(err) => vec![format!("{}: {}", err.code, err.message)],
        },
        lain => vec![format!(
            "subcommand mm '{lain}' tidak dikenal; pakai list, show, add, refresh, rm"
        )],
    }
}

fn advisor(boot: &Boot, state: &mut FeatureState, arg: &str) -> Vec<String> {
    match arg {
        "" => vec![format!(
            "advisor: {} (model: {}, provider: {})",
            if state.advisor_on { "on" } else { "off" },
            if state.advisor_model.is_empty() {
                "sama dengan sesi"
            } else {
                state.advisor_model.as_str()
            },
            if let Some((base, env)) = &state.advisor_provider {
                format!("{base} (kunci {env})")
            } else {
                "sama dengan sesi".to_string()
            }
        )],
        "on" => {
            if boot.config.gateway.api_key_env.is_empty() {
                return vec!["advisor butuh gateway dengan kunci API".to_string()];
            }
            state.advisor_on = true;
            vec!["advisor: on (review setelah turn)".to_string()]
        }
        "off" => {
            state.advisor_on = false;
            vec!["advisor: off".to_string()]
        }
        _ if arg.starts_with("model ") => {
            let model = arg.trim_start_matches("model ").trim();
            if model.is_empty() {
                return vec!["pakai: /advisor model <id>".to_string()];
            }
            state.advisor_model = model.to_string();
            vec![format!("advisor model: {model}")]
        }
        lain => vec![format!(
            "argumen advisor '{lain}' tidak dikenal; pakai on, off, atau model <id>"
        )],
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn pane_memilih_tmux_lalu_manual() {
        let exe = Path::new("/usr/bin/maicli");
        assert!(matches!(
            pane_argv(Some("1"), None, None, exe, "abc", ""),
            PaneCommand::Spawn(argv) if argv[0] == "tmux" && argv[1] == "new-window"
        ));
        assert!(matches!(
            pane_argv(None, None, None, exe, "abc", ""),
            PaneCommand::Manual(teks) if teks.contains("--resume abc")
        ));
    }

    #[test]
    fn preview_memotong_newline() {
        let message = Message::user("baris satu\nbaris dua");
        assert!(!preview(&message).contains('\n'));
    }

    #[test]
    fn guest_join_memvalidasi_url_dan_argumen() {
        assert!(guest_join_parts("join ws://127.0.0.1:7475 roomA kunci").is_some());
        assert!(guest_join_parts("join http://x roomA kunci").is_none());
        assert!(guest_join_parts("join ws://x roomA").is_none());
        assert!(guest_join_parts("start").is_none());
    }

    #[test]
    fn redaksi_menyembunyikan_kredensial_dan_path_user() {
        let teks = "Authorization: Bearer sk-rahasia /home/linux/kerja";
        let hasil = redaksi(teks);
        assert!(!hasil.contains("sk-rahasia"), "{hasil}");
        assert!(hasil.contains("/home/<user>/kerja"), "{hasil}");
    }

    #[test]
    fn todo_markdown_memuat_status() {
        let teks = todo_markdown(&[
            "belum".to_string(),
            "[mulai] jalan".to_string(),
            "[selesai] tuntas".to_string(),
            "[dibuang] buang".to_string(),
        ]);
        assert!(teks.contains("- [ ] belum"), "{teks}");
        assert!(teks.contains("- [>] jalan"), "{teks}");
        assert!(teks.contains("- [x] tuntas"), "{teks}");
        assert!(teks.contains("- [-] buang"), "{teks}");
    }

    #[test]
    fn ringkas_event_anak_memuat_tool_dan_end() {
        let (nama, payload) = ringkas_event_anak(&tui_wire::AgentEvent::ToolExecutionEnd {
            tool_call_id: "c".to_string(),
            tool_name: "bash".to_string(),
            is_error: true,
            preview: "x".to_string(),
        })
        .expect("tool_end");
        assert_eq!(nama, "tool_end");
        assert!(payload.contains(r#""is_error":true"#), "{payload}");
        let (nama, _) =
            ringkas_event_anak(&tui_wire::AgentEvent::AgentEnd { turns: 2 }).expect("agent_end");
        assert_eq!(nama, "agent_end");
    }
    #[test]
    fn diff_review_membaca_perubahan_git() {
        let root = std::env::temp_dir().join(format!(
            "maicli-review-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "T"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(&args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("a.txt"), "awal\n").unwrap();
        for args in [vec!["add", "a.txt"], vec!["commit", "-qm", "awal"]] {
            assert!(
                std::process::Command::new("git")
                    .args(&args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("a.txt"), "ubah\n").unwrap();
        let diff = diff_untuk_review(&root).expect("diff terbaca");
        assert!(diff.contains("a.txt"), "{diff}");
        assert!(diff.contains("ubah"), "{diff}");
        std::fs::remove_dir_all(root).ok();
    }
    #[test]
    fn editor_dari_env_memisah_argumen() {
        assert_eq!(
            editor_dari_env("code -w --goto"),
            Some((
                "code".to_string(),
                vec!["-w".to_string(), "--goto".to_string()]
            ))
        );
        assert_eq!(editor_dari_env("   "), None);
    }
    #[test]
    fn budget_menampilkan_persentase_konteks() {
        let usage = tui_wire::Usage {
            input_tokens: 30,
            output_tokens: 20,
            cached_tokens: 5,
        };
        let teks = format_budget(usage, 100);
        assert!(teks.contains("50/100 (50%)"), "{teks}");
        assert!(teks.contains("sisa: 50"), "{teks}");
    }
    #[test]
    fn daftar_prompt_file_md_saja() {
        let root = std::env::temp_dir().join(format!(
            "maicli-prompts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = prompt_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.md"), "B").unwrap();
        std::fs::write(dir.join("a.md"), "A").unwrap();
        std::fs::write(dir.join("abaikan.txt"), "x").unwrap();
        assert_eq!(daftar_prompt(&root), vec!["a".to_string(), "b".to_string()]);
        std::fs::remove_dir_all(root).ok();
    }
    #[test]
    fn diagnose_menghitung_berkas_berubah() {
        let teks = ringkas_diagnose(" M a.rs\n?? b.rs\n", " a.rs | 2 +-\n b.rs | 1 +\n");
        assert!(teks.contains("2 berkas berubah"), "{teks}");
        assert!(teks.contains("2 baris statistik"), "{teks}");
    }
    #[test]
    fn validasi_nama_skill() {
        assert!(nama_skill_valid("review-kode"));
        assert!(!nama_skill_valid("../luar"));
        assert!(!nama_skill_valid("Spasi"));
    }
    #[test]
    fn xd_issue_parser() {
        assert_eq!(xd_issue_id("xd://issue/issue-1"), Some("issue-1"));
        assert_eq!(xd_issue_id("xd://lain/1"), None);
    }
}
