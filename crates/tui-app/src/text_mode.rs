//! Mode teks: REPL sederhana tanpa terminal interaktif.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use tui_approval::Approver;
use tui_core::TurnOutcome;
use tui_core::{AgentSession, CommandContext, CommandResult, TurnSettings, dispatch};
use tui_session::{SessionEntry, SessionStore};
use tui_tools::{AskRequest, ToolContext, ToolRegistry};
use tui_wire::{AgentEvent, Message, MxError, ProviderEvent};

use crate::Boot;
use crate::approvals::{StdioDecisionUi, build_approver};

fn settings(boot: &Boot) -> TurnSettings {
    TurnSettings {
        model: boot.model.clone(),
        system_prompt: {
            let mut sp = TurnSettings::default().system_prompt;
            sp.push_str(&tui_tools::skills::ringkas_daftar(&boot.skills));
            sp
        },
        deadline: Some(std::time::Duration::from_secs(
            boot.limits.turn_deadline_secs,
        )),
        ..TurnSettings::default()
    }
}

fn tool_context(boot: &Boot, ask: Option<UnboundedSender<AskRequest>>) -> ToolContext {
    let mut ctx = ToolContext::new(boot.cwd.clone());
    ctx.output_cap = boot.limits.tool_output_cap_bytes;
    ctx.interactive = true;
    ctx.bash_timeout = std::time::Duration::from_secs(boot.limits.bash_timeout_secs);
    ctx.bash_max_timeout = std::time::Duration::from_secs(boot.limits.bash_max_timeout_secs);
    ctx.ask = ask;
    ctx
}

fn approver_for(boot: &Boot) -> Result<Arc<dyn Approver>, MxError> {
    let ui = Box::new(StdioDecisionUi::new(std::time::Duration::from_secs(
        boot.limits.approval_timeout_secs,
    )));
    Ok(Arc::new(build_approver(
        boot.approval.clone(),
        ui,
        boot.grants_path.clone(),
        boot.grant_ttl,
    )?))
}

fn build_session(
    boot: &Boot,
    events: UnboundedSender<AgentEvent>,
    registry: ToolRegistry,
    approver: Arc<dyn Approver>,
    ask: Option<UnboundedSender<AskRequest>>,
) -> AgentSession {
    let mut session = AgentSession::new(
        Arc::new(boot.client.clone()),
        approver,
        registry,
        tool_context(boot, ask),
        settings(boot),
        events,
    );
    session.set_context_window(boot.limits.context_window_tokens);
    session
}

/// Cetak event agent ke stdout selama sesi REPL berjalan.
async fn print_events(mut events: UnboundedReceiver<AgentEvent>) {
    use std::io::Write;
    let mut line_open = false;
    while let Some(event) = events.recv().await {
        match event {
            AgentEvent::MessageUpdate(ProviderEvent::TextDelta { text }) => {
                print!("{text}");
                let _ = std::io::stdout().flush();
                line_open = true;
            }
            AgentEvent::MessageUpdate(ProviderEvent::TextEnd) => {
                if line_open {
                    println!();
                    line_open = false;
                }
            }
            AgentEvent::ToolExecutionStart {
                tool_name, preview, ..
            } => {
                println!("→ {tool_name}: {preview}");
            }
            AgentEvent::ToolExecutionEnd {
                tool_name,
                is_error,
                preview,
                ..
            } => {
                let mark = if is_error { "✗" } else { "✓" };
                println!("{mark} {tool_name}: {preview}");
            }
            AgentEvent::Notice { text } => println!("! {text}"),
            _ => {}
        }
    }
}

fn store_targets(boot: &Boot) -> (std::path::PathBuf, Option<std::path::PathBuf>, String) {
    (
        boot.sessions_dir.clone(),
        boot.resume_path.clone(),
        boot.cwd.display().to_string(),
    )
}

async fn persist(store: &mut Option<SessionStore>, message: &Message) {
    crate::session_open::append(store, SessionEntry::message(None, message.clone())).await;
}

/// Simpan ringkasan compaction ke transkrip supaya resume memakai konteks yang
/// sudah diringkas, bukan pesan lama.
async fn persist_compactions(store: &mut Option<SessionStore>, session: &mut AgentSession) {
    for record in session.take_compactions() {
        crate::session_open::append(
            store,
            SessionEntry::compaction(None, record.summary, record.replaced),
        )
        .await;
    }
    for audit in session.take_approvals() {
        crate::session_open::append(
            store,
            SessionEntry::approval(
                audit.tool,
                format!("{:?}", audit.tier).to_lowercase(),
                audit.subject,
                audit.decision,
            ),
        )
        .await;
    }
}

/// Jalankan satu prompt lalu keluar.
pub async fn run_once(mut boot: Boot, prompt: String) -> Result<(), MxError> {
    if boot.approval.mode == tui_approval::ApprovalMode::Yolo {
        println!("{}", crate::banner::yolo_banner());
    }
    for note in &boot.mcp_notes {
        println!("[mcp] {note}");
    }
    let (tx, rx, event_log_done) = crate::event_log::channel(boot.event_log.clone());
    // Penerus notifikasi: task berhenti sendiri setelah 3×2 dtk idle (tanpa deadlock).
    let notif_task = boot
        .notif_rx
        .take()
        .map(|rx| crate::mcp_boot::spawn_event_forward(rx, tx.clone()));
    let printer = tokio::spawn(print_events(rx));
    let approver = approver_for(&boot)?;
    let mut session = build_session(&boot, tx, boot.tool_registry(), approver, None);
    session.seed_usage(boot.resume_usage);
    session.push_messages(boot.resume_messages.clone());
    let mut persisted_usage = boot.resume_usage;
    let (dir, resume, cwd) = store_targets(&boot);
    let mut store = match crate::session_open::open_or_create(dir, resume, cwd).await {
        Ok(store) => Some(store),
        Err(err) => {
            println!("sesi tidak tersimpan: {}: {}", err.code, err.message);
            None
        }
    };
    persist(&mut store, &Message::user(prompt.clone())).await;
    let outcome = session.run_turn(Some(prompt)).await?;
    persist_compactions(&mut store, &mut session).await;
    crate::usage::persist_usage(&mut store, &session, &mut persisted_usage).await;
    if let Some(last) = session
        .messages()
        .iter()
        .rev()
        .find(|m| matches!(m, Message::Assistant { .. }))
    {
        persist(&mut store, last).await;
    }
    drop(session);
    if let Some(tugas) = notif_task {
        tugas.abort();
    }
    let _ = printer.await;
    crate::event_log::flush(event_log_done).await;
    println!(
        "\n[selesai: {} turn, {:?}]",
        outcome.turns, outcome.stop_reason
    );
    Ok(())
}

/// Pembaca stdin di thread terpisah.
///
/// Baris dikirim ke kanal supaya REPL dan tool `ask` bisa berbagi stdin tanpa
/// saling mengunci; thread berhenti saat stdin EOF atau kanal ditutup.
fn spawn_stdin_reader(tx: UnboundedSender<String>) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// Arahkan satu baris stdin: jawab pertanyaan aktif, atau simpan untuk prompt
/// berikutnya. Mengembalikan pesan yang perlu dicetak, bila ada.
fn route_line(
    line: String,
    pending: &mut Option<(String, oneshot::Sender<String>)>,
    queued: &mut VecDeque<String>,
) -> Option<String> {
    let Some((question, reply)) = pending.take() else {
        queued.push_back(line);
        return None;
    };
    let answer = line.trim().to_string();
    let shown = if answer.is_empty() {
        "(dilewati)".to_string()
    } else {
        answer.clone()
    };
    match reply.send(answer) {
        Ok(()) => Some(format!("[tanya] {question} → {shown}")),
        Err(_) => Some(format!("[tanya] kedaluwarsa sebelum dijawab: {question}")),
    }
}

/// Tangani baris yang diketik saat turn masih berjalan.
///
/// Baris yang menjawab `ask` diproses seperti biasa; baris lain **ditolak**
/// dengan pesan jelas agar konsisten dengan TUI/RPC (RENCANA C-03: prompt saat
/// sibuk ditolak, bukan diantrekan diam-diam).
fn line_saat_sibuk(
    line: String,
    pending: &mut Option<(String, oneshot::Sender<String>)>,
    queued: &mut VecDeque<String>,
) -> Option<String> {
    match route_line(line, pending, queued) {
        Some(message) => Some(message),
        None => {
            // route_line menitipkan baris ke antrean; buang karena ditolak.
            let _ = queued.pop_back();
            Some("masih sibuk; kirim ulang setelah turn ini selesai".to_string())
        }
    }
}

/// Apakah baris adalah perintah keluar (`/quit` atau alias `/exit`).
fn is_quit_command(line: &str) -> bool {
    matches!(line.trim(), "/quit" | "/exit")
}

/// Hasil satu tekanan Ctrl+C saat idle.
#[derive(Debug, PartialEq, Eq)]
enum IdleCtrl {
    /// Tekanan pertama: minta konfirmasi.
    Armed,
    /// Tekanan kedua: keluar.
    Quit,
}

/// Tunggu Ctrl+C berturut-turut saat idle (tekanan pertama hanya mempersenjatai).
fn idle_ctrl(armed: &mut bool) -> IdleCtrl {
    if *armed {
        IdleCtrl::Quit
    } else {
        *armed = true;
        IdleCtrl::Armed
    }
}

/// Jalankan satu turn sambil melayani pertanyaan `ask` dan baris yang diketik
/// lebih awal (baris itu disimpan untuk prompt berikutnya, bukan dibuang).
async fn turn_with_ask(
    session: &mut AgentSession,
    text: String,
    ask_rx: &mut UnboundedReceiver<AskRequest>,
    lines_rx: &mut UnboundedReceiver<String>,
    ctrl_rx: &mut UnboundedReceiver<()>,
    queued: &mut VecDeque<String>,
) -> Result<TurnOutcome, MxError> {
    let mut pending: Option<(String, oneshot::Sender<String>)> = None;
    let token = session.cancel_token();
    let run = session.run_turn(Some(text));
    tokio::pin!(run);
    loop {
        tokio::select! {
            result = &mut run => return result,
            Some(()) = ctrl_rx.recv() => {
                token.cancel();
                println!("[batal] turn dibatalkan (Ctrl+C)");
            }
            Some(request) = ask_rx.recv() => {
                println!("\n[tanya] {}", request.question);
                println!("jawab: ketik balasan lalu Enter (kosong = lewati)");
                pending = Some((request.question, request.reply));
            }
            line = lines_rx.recv() => {
                if let Some(line) = line {
                    // TUI menunda `/quit` saat sibuk: batalkan turn dulu, bukan
                    // menolak perintah keluar. Kecuali baris itu jawaban `ask`.
                    if pending.is_none() && is_quit_command(&line) {
                        token.cancel();
                        println!(
                            "[batal] /quit saat turn berjalan: turn dibatalkan; jalankan /quit setelah idle"
                        );
                    } else if let Some(message) =
                        line_saat_sibuk(line, &mut pending, queued)
                    {
                        println!("{message}");
                    }
                }
            }
        }
    }
}

/// Daftar sesi tersimpan, terbaru lebih dulu.
fn print_sessions(boot: &Boot, query: Option<&str>) {
    let sessions = match query {
        Some(query) => tui_session::search(&boot.sessions_dir, query),
        None => tui_session::list(&boot.sessions_dir),
    };
    if sessions.is_empty() {
        println!("belum ada sesi di {}", boot.sessions_dir.display());
        return;
    }
    for summary in sessions {
        println!("{}", summary.display_line());
    }
}

/// REPL interaktif.
pub async fn run(mut boot: Boot) -> Result<(), MxError> {
    if boot.approval.mode == tui_approval::ApprovalMode::Yolo {
        println!("{}", crate::banner::yolo_banner());
    }
    let (tx, rx, event_log_done) = crate::event_log::channel(boot.event_log.clone());
    // Penerus notifikasi: idle-timeout mencegah deadlock printer.
    let notif_task = boot
        .notif_rx
        .take()
        .map(|rx| crate::mcp_boot::spawn_event_forward(rx, tx.clone()));
    let printer = tokio::spawn(print_events(rx));
    let registry = boot.tool_registry();
    let tool_names = registry.names();
    let approver = approver_for(&boot)?;
    let (ask_tx, mut ask_rx) = unbounded_channel::<AskRequest>();
    let events_for_switch = tx.clone();
    let approver_for_switch = approver.clone();
    let mut session = build_session(&boot, tx, registry, approver, Some(ask_tx.clone()));
    session.seed_usage(boot.resume_usage);
    session.push_messages(boot.resume_messages.clone());
    let mut persisted_usage = boot.resume_usage;
    if let Some(path) = boot.resume_path.as_ref() {
        println!(
            "melanjutkan sesi {} ({} pesan)",
            path.display(),
            boot.resume_messages.len()
        );
    }
    let (dir, resume, cwd) = store_targets(&boot);
    let mut store = match crate::session_open::open_or_create(dir, resume, cwd).await {
        Ok(store) => Some(store),
        Err(err) => {
            println!("sesi tidak tersimpan: {}: {}", err.code, err.message);
            None
        }
    };
    match store.as_ref() {
        Some(store) => println!("sesi: {}", store.path().display()),
        None => println!("sesi: tidak tersimpan (direktori sesi tidak bisa dibuat)"),
    }
    for note in &boot.mcp_notes {
        println!("[mcp] {note}");
    }
    println!("ketik /help untuk daftar perintah.");
    let (lines_tx, mut lines_rx) = unbounded_channel::<String>();
    spawn_stdin_reader(lines_tx);
    // Ctrl+C pada Windows akan menghentikan proses secara default; handler ini
    // membuatnya menjadi sinyal yang bisa dipakai membatalkan turn (seperti TUI).
    let (ctrl_tx, mut ctrl_rx) = unbounded_channel::<()>();
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }
            if ctrl_tx.send(()).is_err() {
                break;
            }
        }
    });
    let mut ctrl_armed = false;
    let mut queued: VecDeque<String> = VecDeque::new();
    loop {
        let line = match queued.pop_front() {
            Some(line) => line,
            None => {
                print!("\n> ");
                use std::io::Write;
                let _ = std::io::stdout().flush();
                tokio::select! {
                    maybe = lines_rx.recv() => match maybe {
                        Some(line) => line,
                        None => break,
                    },
                    Some(()) = ctrl_rx.recv() => {
                        match idle_ctrl(&mut ctrl_armed) {
                            IdleCtrl::Armed => {
                                println!("\ntekan Ctrl+C sekali lagi untuk keluar.");
                            }
                            IdleCtrl::Quit => {
                                println!("\nkeluar.");
                                break;
                            }
                        }
                        continue;
                    }
                }
            }
        };
        let text = line.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let session_path = store.as_ref().map(|store| store.path().to_path_buf());
        let context = CommandContext {
            model: session.model(),
            provider: (&boot.provider.0, &boot.provider.1),
            tools: &tool_names,
            session_path: session_path.as_deref(),
            known_models: &boot.models,
            sessions_dir: Some(&boot.sessions_dir),
        };
        match dispatch(&text, &context) {
            CommandResult::Quit => break,
            CommandResult::Reply(reply) => {
                println!("{reply}");
                continue;
            }
            CommandResult::SetModel(model) => {
                session.set_model(model.clone());
                crate::session_open::append(&mut store, SessionEntry::model_change(model.clone()))
                    .await;
                println!("model aktif: {model}");
                continue;
            }
            CommandResult::Sessions(query) => {
                print_sessions(&boot, query.as_deref());
                continue;
            }
            CommandResult::Tree => {
                // Pohon sesi (fork): indentasi per kedalaman.
                println!("pohon sesi:");
                for simpul in tui_session::pohon(&boot.sessions_dir) {
                    println!("{}", simpul.display_line());
                }
                continue;
            }
            CommandResult::NewSession => {
                let dir = boot.sessions_dir.clone();
                let cwd = boot.cwd.display().to_string();
                match crate::session_open::create_new(dir, cwd).await {
                    Ok(new_store) => {
                        let path = new_store.path().display().to_string();
                        let mut fresh = build_session(
                            &boot,
                            events_for_switch.clone(),
                            boot.tool_registry(),
                            approver_for_switch.clone(),
                            Some(ask_tx.clone()),
                        );
                        fresh.set_context_window(boot.limits.context_window_tokens);
                        session = fresh;
                        store = Some(new_store);
                        persisted_usage = tui_wire::Usage::default();
                        println!("sesi baru: {path}");
                    }
                    Err(err) => println!("{}: {}", err.code, err.message),
                }
                continue;
            }
            CommandResult::ForkSession => {
                let Some(current) = store.as_ref().map(|store| store.path().to_path_buf()) else {
                    println!("belum ada sesi tersimpan untuk diduplikasi");
                    continue;
                };
                let dir = boot.sessions_dir.clone();
                match crate::session_open::fork_current(
                    current,
                    dir,
                    "sesi maicli (fork)".to_string(),
                )
                .await
                {
                    Ok((new_store, messages)) => {
                        let path = new_store.path().display().to_string();
                        let count = messages.len();
                        let mut fresh = build_session(
                            &boot,
                            events_for_switch.clone(),
                            boot.tool_registry(),
                            approver_for_switch.clone(),
                            Some(ask_tx.clone()),
                        );
                        fresh.set_context_window(boot.limits.context_window_tokens);
                        fresh.push_messages(messages);
                        session = fresh;
                        store = Some(new_store);
                        persisted_usage = session.usage();
                        println!("fork sesi: {path} ({count} pesan)");
                    }
                    Err(err) => println!("{}: {}", err.code, err.message),
                }
                continue;
            }
            CommandResult::ForgetApprovals => {
                approver_for_switch.forget_grants();
                println!("grant approval dihapus (file grants.json dikosongkan)");
                continue;
            }
            CommandResult::SetProvider(Some(id)) => {
                // Config v1 tidak melakukan file-watching: ganti provider berarti
                // tulis config global lalu minta restart.
                match tui_config::set_global_value(
                    tui_config::default_global_path()
                        .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
                        .as_path(),
                    "gateway.provider",
                    &id,
                ) {
                    Ok(()) => println!(
                        "provider diset ke '{id}' (config global); restart aplikasi agar berlaku"
                    ),
                    Err(err) => println!("{}: {}", err.code, err.message),
                }
                continue;
            }
            CommandResult::SetProvider(None) => continue,
            CommandResult::Context => {
                println!(
                    "{}",
                    tui_core::commands::context_text(session.estimated_tokens(), session.budget())
                );
                continue;
            }
            CommandResult::Todo => {
                println!("{}", tui_core::commands::todo_text(&session.todos()));
                continue;
            }
            CommandResult::SetTitle(new_title) => {
                if store.is_some() {
                    crate::session_open::append(
                        &mut store,
                        SessionEntry::title_change(new_title.clone()),
                    )
                    .await;
                    println!("judul sesi: {new_title}");
                } else {
                    println!("belum ada sesi tersimpan untuk diberi judul");
                }
                continue;
            }
            CommandResult::SwitchSession(target) => {
                let wanted = target.unwrap_or_else(|| "latest".to_string());
                let sessions_dir = boot.sessions_dir.clone();
                let opened = tokio::task::spawn_blocking(move || {
                    let summary = tui_session::resolve_target(&sessions_dir, &wanted)?;
                    let loaded = tui_session::SessionStore::load(&summary.path)?;
                    let messages = loaded.messages();
                    let store = tui_session::SessionStore::open(&summary.path)?;
                    Ok::<_, MxError>((summary, messages, store))
                })
                .await;
                match opened {
                    Ok(Ok((summary, messages, new_store))) => {
                        let count = messages.len();
                        let mut fresh = build_session(
                            &boot,
                            events_for_switch.clone(),
                            boot.tool_registry(),
                            approver_for_switch.clone(),
                            Some(ask_tx.clone()),
                        );
                        fresh.push_messages(messages);
                        session = fresh;
                        store = Some(new_store);
                        println!("pindah ke sesi {} ({count} pesan)", summary.path.display());
                    }
                    Ok(Err(err)) => println!("{}: {}", err.code, err.message),
                    Err(_) => println!("gagal membuka sesi (task batal)"),
                }
                continue;
            }
            CommandResult::Usage => {
                println!(
                    "{}",
                    tui_core::commands::usage_text(
                        session.usage(),
                        session.estimated_tokens(),
                        session.usage_by_model(),
                    )
                );
                continue;
            }
            CommandResult::Pause => {
                session.pause();
                println!("sesi dijeda");
                continue;
            }
            CommandResult::Resume => {
                session.resume();
                println!("sesi dilanjutkan");
                continue;
            }
            CommandResult::Compact => {
                match session.compact_now().await {
                    Some(report) => println!("{}", tui_core::commands::compaction_text(report)),
                    None => println!("tidak ada yang bisa diringkas"),
                }
                continue;
            }
            CommandResult::Unknown(message) => {
                println!("{message}");
                continue;
            }
            CommandResult::NotACommand => {}
        }
        persist(&mut store, &Message::user(text.clone())).await;
        let turn = turn_with_ask(
            &mut session,
            text,
            &mut ask_rx,
            &mut lines_rx,
            &mut ctrl_rx,
            &mut queued,
        )
        .await;
        match turn {
            Ok(outcome) => println!("\n[{} turn, {:?}]", outcome.turns, outcome.stop_reason),
            Err(err) => println!("\n{}: {}", err.code, err.message),
        }
        persist_compactions(&mut store, &mut session).await;
        crate::usage::persist_usage(&mut store, &session, &mut persisted_usage).await;
        if let Some(last) = session
            .messages()
            .iter()
            .rev()
            .find(|m| matches!(m, Message::Assistant { .. }))
        {
            persist(&mut store, last).await;
        }
    }
    drop(session);
    if let Some(tugas) = notif_task {
        tugas.abort();
    }
    let _ = printer.await;
    crate::event_log::flush(event_log_done).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Test gagal keras bila asumsi tidak berlaku.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn quit_saat_sibuk_memicu_pembatalan() {
        assert!(is_quit_command("/quit"));
        assert!(is_quit_command("  /exit "));
        assert!(!is_quit_command("/quit now"));
        assert!(!is_quit_command("halo"));
    }

    #[test]
    fn ctrl_c_idle_butuh_dua_tekanan() {
        let mut armed = false;
        assert_eq!(idle_ctrl(&mut armed), IdleCtrl::Armed);
        assert_eq!(idle_ctrl(&mut armed), IdleCtrl::Quit);
    }

    #[test]
    fn baris_saat_sibuk_ditolak_bukan_diantrekan() {
        let mut pending = None;
        let mut queued = VecDeque::new();
        let message =
            line_saat_sibuk("halo".to_string(), &mut pending, &mut queued).expect("pesan");
        assert!(message.contains("masih sibuk"), "{message}");
        assert!(queued.is_empty(), "baris ditolak tidak boleh masuk antrean");
    }

    #[test]
    fn balasan_ask_saat_sibuk_tetap_diproses() {
        let (reply, answer) = oneshot::channel();
        let mut pending = Some(("lanjut?".to_string(), reply));
        let mut queued = VecDeque::new();
        let message = line_saat_sibuk("ya".to_string(), &mut pending, &mut queued).expect("pesan");
        assert!(message.contains("ya"), "{message}");
        assert_eq!(answer.blocking_recv().expect("jawaban"), "ya");
        assert!(queued.is_empty());
    }

    #[test]
    fn route_line_menjawab_pertanyaan_aktif() {
        let (reply, answer) = oneshot::channel();
        let mut pending = Some(("lanjut?".to_string(), reply));
        let mut queued = VecDeque::new();
        let message =
            route_line("  ya  ".to_string(), &mut pending, &mut queued).expect("ada pesan");
        assert!(message.contains("lanjut?"), "{message}");
        assert!(message.contains("ya"), "{message}");
        assert_eq!(answer.blocking_recv().expect("jawaban"), "ya");
        assert!(queued.is_empty());
        assert!(pending.is_none());
    }

    #[test]
    fn route_line_menyimpan_baris_tanpa_pertanyaan() {
        let mut pending = None;
        let mut queued = VecDeque::new();
        assert!(route_line("halo".to_string(), &mut pending, &mut queued).is_none());
        assert_eq!(queued.pop_front().as_deref(), Some("halo"));
    }

    #[test]
    fn route_line_jawaban_kosong_dilewati_dan_kedaluwarsa_dicatat() {
        let (reply, answer) = oneshot::channel();
        drop(answer);
        let mut pending = Some(("lanjut?".to_string(), reply));
        let mut queued = VecDeque::new();
        let message = route_line(String::new(), &mut pending, &mut queued).expect("pesan");
        assert!(message.contains("kedaluwarsa"), "{message}");
    }
}
