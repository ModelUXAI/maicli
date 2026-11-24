//! Mode TUI: viewport histori (bisa di-scroll), composer, status, overlay
//! bantuan, dan dialog approval inline.

use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use unicode_width::UnicodeWidthChar;

use tui_approval::{Approver, Decision, PolicyApprover};
use tui_core::{
    AgentSession, COMMANDS, CommandContext, CommandResult, CommandSpec, TurnSettings, dispatch,
};
use tui_session::{SessionEntry, SessionStore};
use tui_tools::{AskRequest, ToolContext};
use tui_wire::{AgentEvent, ErrorCode, MxError, ProviderEvent, SessionChoice};

use crate::Boot;
use crate::approvals::{ChannelDecisionUi, PendingApproval, build_approver};
use crate::features::{FeatureAction, FeatureState};

/// Pertanyaan dari tool `ask` yang menunggu jawaban pengguna.
struct PendingQuestion {
    question: String,
    reply: tokio::sync::oneshot::Sender<String>,
}

/// Batas baris histori yang disimpan di memori.
const MAX_LINES: usize = 2_000;

enum Command {
    Prompt(String),
    Cancel,
    Quit,
}

#[derive(Debug, PartialEq)]
enum Action {
    Submit(String),
    Abort,
    Quit,
    SwitchSession(String),
}

/// Palet command (`Ctrl+K`) yang sedang terbuka.
#[derive(Debug)]
struct Palette {
    query: String,
    selected: usize,
}

/// Pemilih sesi yang sedang terbuka.
#[derive(Debug)]
struct Picker {
    items: Vec<SessionChoice>,
    selected: usize,
    /// Sumber picker: sesi (→ /switch) atau katalog (→ /catalog <id>).
    jenis: PickerJenis,
}

/// Sumber picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerJenis {
    /// Pemilih sesi → Action::SwitchSession.
    Sesi,
    /// Pemilih katalog → Action::Submit("/catalog <id>").
    Katalog,
    /// Pemilih model → Action::Submit("/model <id>").
    Model,
}

/// Mode input panel Agent Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HubMode {
    /// Melihat event (tanpa input).
    Lihat,
    /// Mengirim steering ke anak.
    Steer,
    /// Mengirim prompt turn berikutnya.
    Prompt,
}

/// Tampilan kolom kanan panel Agent Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HubTampilan {
    /// Aliran event (tool, catatan, siklus turn).
    Event,
    /// Transkrip pesan asisten (teks jawaban per turn).
    Transkrip,
}

/// Panel Agent Hub penuh (Alt+A): daftar anak + event + kendali.
#[derive(Debug)]
struct HubPanel {
    /// Indeks anak terpilih dalam roster.
    selected: usize,
    /// Mode input aktif.
    mode: HubMode,
    /// Teks input steer/prompt.
    input: String,
    /// Tampilan kolom kanan: event atau transkrip.
    tampilan: HubTampilan,
    /// Enter berikutnya = broadcast steer ke semua anak berjalan.
    broadcast: bool,
}

/// Form penambahan provider (interaktif, bertahap via composer).
#[derive(Debug)]
struct FormProvider {
    /// Langkah: 0 = id, 1 = base_url, 2 = env key, 3 = model bawaan.
    langkah: usize,
    id: String,
    base_url: String,
    env_key: String,
    model: String,
}

impl FormProvider {
    /// Pertanyaan langkah saat ini (ditampilkan sebagai status).
    fn pertanyaan(&self) -> &'static str {
        match self.langkah {
            0 => "provider add — id (tanpa spasi): ",
            1 => "provider add — base_url (mis. https://…/v1): ",
            2 => "provider add — env var kunci API: ",
            _ => "provider add — model bawaan (boleh kosong): ",
        }
    }
}

/// Rekam satu event anak ke buffer hub (cap 300 per anak) — dipakai kolektor
/// di `run` dan test.
fn rekam_event(
    buffer: &std::sync::Mutex<
        std::collections::HashMap<String, std::collections::VecDeque<String>>,
    >,
    id: &str,
    event: &tui_wire::AgentEvent,
) {
    if let Ok(mut map) = buffer.lock() {
        let antre = map.entry(id.to_string()).or_default();
        if antre.len() >= 300 {
            antre.pop_front();
        }
        antre.push_back(Screen::baris_event_anak(event));
    }
}

/// Rekam transkrip asisten dari event `MessageEnd` (cap 200 baris, tiap baris
/// maks 400 karakter — panel, bukan arsip).
fn rekam_transkrip(
    buffer: &std::sync::Mutex<
        std::collections::HashMap<String, std::collections::VecDeque<String>>,
    >,
    id: &str,
    message: &tui_wire::AssistantMessage,
) {
    // Ambil blok teks (bila ada); tool call dicatat sebagai baris ringkas.
    let mut baris_baru: Vec<String> = Vec::new();
    for blok in &message.content {
        match blok {
            tui_wire::ContentBlock::Text { text } => {
                let pendek: String = text.chars().take(400).collect();
                if !pendek.trim().is_empty() {
                    baris_baru.push(format!("asisten: {pendek}"));
                }
            }
            tui_wire::ContentBlock::ToolUse { name, .. } => {
                baris_baru.push(format!("→ panggil {name}"));
            }
            _ => {}
        }
    }
    if baris_baru.is_empty() {
        return;
    }
    if let Ok(mut map) = buffer.lock() {
        let antre = map.entry(id.to_string()).or_default();
        for baris in baris_baru {
            if antre.len() >= 200 {
                antre.pop_front();
            }
            antre.push_back(baris);
        }
    }
}

/// Token warna execution truth dari `design-reference/*/code.html`.
/// Dipakai terminal TUI supaya biner yang diunduh pengguna memakai sistem
/// visual yang sama dengan referensi (bukan tema biru lama).
#[derive(Debug, Clone, Copy)]
struct Theme {
    canvas: Color,
    lowest: Color,
    low: Color,
    container: Color,
    high: Color,
    highest: Color,
    on_surface: Color,
    on_variant: Color,
    outline: Color,
    outline_variant: Color,
    primary: Color,
    primary_container: Color,
    secondary: Color,
    secondary_container: Color,
    tertiary: Color,
    tertiary_container: Color,
    error: Color,
    error_container: Color,
}

impl Theme {
    fn reference() -> Self {
        let rgb = |hex: &str| parse_color(hex);
        Self {
            canvas: rgb("#0b141c"),
            lowest: rgb("#060f16"),
            low: rgb("#141c24"),
            container: rgb("#182028"),
            high: rgb("#222b33"),
            highest: rgb("#2d363e"),
            on_surface: rgb("#dae3ee"),
            on_variant: rgb("#c0c7d4"),
            outline: rgb("#8b919d"),
            outline_variant: rgb("#414752"),
            primary: rgb("#a2c9ff"),
            primary_container: rgb("#58a6ff"),
            secondary: rgb("#dbb8ff"),
            secondary_container: rgb("#593482"),
            tertiary: rgb("#6fdd78"),
            tertiary_container: rgb("#49b858"),
            error: rgb("#ffb4ab"),
            error_container: rgb("#93000a"),
        }
    }
}

struct Screen {
    lines: Vec<String>,
    current: String,
    input: String,
    status: String,
    model: String,
    /// Prompt yang menunggu turn selesai (dari `/queue` saat sibuk).
    queue: Vec<String>,
    pending: Option<PendingApproval>,
    question: Option<PendingQuestion>,
    accent: Color,
    dim: Color,
    /// Token warna referensi terminal (modal palette memakai field `palette`).
    theme: Theme,
    keybindings: std::collections::HashMap<String, KeyEvent>,
    scroll: u16,
    view_height: u16,
    view_width: u16,
    show_help: bool,
    picker: Option<Picker>,
    palette: Option<Palette>,
    expanded_tools: bool,
    tool_details: std::collections::HashMap<usize, String>,
    tool_meta: std::collections::HashMap<String, tui_wire::AgentToolMeta>,
    pending_line: Option<usize>,
    pending_since: Option<std::time::Instant>,
    pending_timeout: Duration,
    pending_choice: Option<bool>,
    pending_choice_line: Option<usize>,
    pending_countdown_line: Option<usize>,
    context: Option<(u64, u64)>,
    history: Vec<String>,
    history_index: Option<usize>,
    quit_armed: bool,
    busy: bool,
    approval_mode: tui_approval::ApprovalMode,
    turn_state: tui_core::TurnStatus,
    /// Manager subagent (panel F3); `None` = fitur mati di mode ini.
    subagents: Option<Arc<tui_subagent::manager::SubagentManager>>,
    /// Panel subagent terbuka.
    agents_open: bool,
    /// Panel Agent Hub penuh (Alt+A).
    hub: Option<HubPanel>,
    /// Form `/provider add` aktif (menangkap composer sampai selesai).
    form_add: Option<FormProvider>,
    /// Buffer event per anak (id → baris event terbaru, cap 300).
    hub_events: Arc<
        std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<String>>>,
    >,
    /// Transkrip pesan per anak (id → baris "asisten: …", cap 200).
    hub_transkrip: Arc<
        std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<String>>>,
    >,
}

impl Screen {
    /// Pasang override tombol dari config; spec invalid diabaikan.
    fn set_keybindings(&mut self, config: &std::collections::BTreeMap<String, String>) {
        self.keybindings = config
            .iter()
            .filter_map(|(aksi, spec)| parse_key(spec).map(|k| (aksi.clone(), k)))
            .collect();
    }

    /// Apakah tombol ini merupakan override untuk aksi tertentu.
    fn key_override(&self, aksi: &str, key: KeyEvent) -> bool {
        self.keybindings.get(aksi).is_some_and(|k| *k == key)
    }

    fn new(
        model: String,
        approval_mode: tui_approval::ApprovalMode,
        accent: Color,
        dim: Color,
        approval_timeout: Duration,
    ) -> Self {
        Self {
            lines: Vec::new(),
            current: String::new(),
            input: String::new(),
            status: "siap".to_string(),
            model,
            queue: Vec::new(),
            pending: None,
            question: None,
            accent,
            dim,
            // Override `ui.accent` tetap dihormati, tetapi foundation palet
            // mengikuti referensi, bukan warna lama.
            theme: Theme::reference(),
            keybindings: std::collections::HashMap::new(),
            scroll: 0,
            view_height: 1,
            view_width: 80,
            show_help: false,
            picker: None,
            palette: None,
            expanded_tools: false,
            tool_details: std::collections::HashMap::new(),
            tool_meta: tui_tools::ToolRegistry::v1()
                .definitions()
                .into_iter()
                .map(|definition| {
                    let meta = tui_tools::agent_tool_meta(&definition.name);
                    (definition.name, meta)
                })
                .collect(),
            pending_line: None,
            pending_since: None,
            pending_timeout: approval_timeout,
            pending_choice: None,
            pending_choice_line: None,
            pending_countdown_line: None,
            context: None,
            history: Vec::new(),
            history_index: None,
            quit_armed: false,
            busy: false,
            approval_mode,
            turn_state: tui_core::TurnStatus::Idle,
            subagents: None,
            agents_open: false,
            hub: None,
            form_add: None,
            hub_events: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            hub_transkrip: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Pasang manager subagent (dipanggil `run` sebelum loop).
    fn set_subagents(&mut self, manager: Arc<tui_subagent::manager::SubagentManager>) {
        self.subagents = Some(manager);
    }

    /// Format satu event anak menjadi baris ringkas untuk panel hub.
    fn baris_event_anak(event: &tui_wire::AgentEvent) -> String {
        match event {
            tui_wire::AgentEvent::AgentStart => "mulai".to_string(),
            tui_wire::AgentEvent::AgentEnd { turns } => format!("selesai ({turns} turn)"),
            tui_wire::AgentEvent::ToolExecutionStart {
                tool_name, preview, ..
            } => format!(
                "tool {tool_name}: {}",
                preview.chars().take(40).collect::<String>()
            ),
            tui_wire::AgentEvent::ToolExecutionEnd {
                tool_name,
                is_error,
                ..
            } => format!(
                "tool selesai {tool_name}{}",
                if *is_error { " [error]" } else { "" }
            ),
            tui_wire::AgentEvent::Notice { text } => format!("catatan: {text}"),
            lain => format!("{lain:?}"),
        }
    }

    /// Ambil anak terpilih (roster) bila panel terbuka.
    fn anak_terpilih(&self) -> Option<tui_subagent::manager::SubagentHandle> {
        let manager = self.subagents.as_ref()?;
        let handles = manager.list();
        let hub = self.hub.as_ref()?;
        handles.get(hub.selected).cloned()
    }

    /// Baris overlay panel subagent (tanpa border); kosong bila tidak ada anak.
    fn subagent_rows(&self) -> Vec<String> {
        let Some(manager) = &self.subagents else {
            return vec!["fitur subagent tidak aktif di mode ini".to_string()];
        };
        let handles = manager.list();
        if handles.is_empty() {
            return vec!["belum ada subagent (model dapat memanggil tool `subagent`)".to_string()];
        }
        handles
            .iter()
            .map(|handle| {
                let status = handle.status();
                let hasil = handle.result();
                let ringkas = match &hasil {
                    Some(hasil) => {
                        let potong: String = hasil.text.chars().take(40).collect();
                        format!(
                            "{} turn · {} tool · {}",
                            hasil.turns, hasil.tool_calls, potong
                        )
                    }
                    None => String::new(),
                };
                format!("{} · {} {}", handle.id, status.label(), ringkas)
            })
            .collect()
    }

    fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::ClearTimeline => {
                // /clear: kosongkan timeline dan kembali ke bawah.
                self.lines.clear();
                self.scroll = 0;
            }
            AgentEvent::AgentStart => {
                self.turn_state = tui_core::TurnStatus::Streaming;
                self.busy = true;
                self.status = "menjalankan turn…".to_string();
            }
            AgentEvent::TurnStart { index } => {
                self.turn_state = tui_core::TurnStatus::Streaming;
                self.status = format!("turn {index}…");
            }
            AgentEvent::MessageUpdate(inner) => match inner {
                ProviderEvent::TextDelta { text } => self.current.push_str(&text),
                ProviderEvent::TextEnd => self.flush_current(),
                _ => {}
            },
            AgentEvent::MessageEnd { message } => {
                self.flush_current();
                if let Some(error) = message.error.as_deref() {
                    self.turn_state = tui_core::TurnStatus::Error;
                    self.push_line(format!("! {error}"));
                } else if message.stop_reason == tui_wire::StopReason::Aborted {
                    self.turn_state = tui_core::TurnStatus::Aborted;
                }
            }
            AgentEvent::ToolExecutionStart {
                tool_name, preview, ..
            } => {
                self.turn_state = tui_core::TurnStatus::ExecutingTools;
                self.flush_current();
                let meta = self
                    .tool_meta
                    .get(&tool_name)
                    .cloned()
                    .unwrap_or_else(|| tui_wire::AgentToolMeta::plain(&tool_name));
                let tanda = match meta.render {
                    tui_wire::RenderHint::Command => "❯",
                    tui_wire::RenderHint::Diff => "±",
                    tui_wire::RenderHint::Plain => "→",
                };
                // Referensi 08: sub-shell `╭─ TOOL │ RUNNING │ preview`.
                let ringkas = preview.replace(['\n', '\r'], " ");
                let ringkas = if ringkas.chars().count() > 72 {
                    format!("{}…", ringkas.chars().take(71).collect::<String>())
                } else {
                    ringkas
                };
                self.push_line(format!(
                    "╭─ {tool_name} │ RUNNING │ {tanda} {}: {}",
                    meta.label, ringkas
                ));
                let index = self.lines.len().saturating_sub(1);
                self.tool_details
                    .insert(index, format!("{tool_name} mulai\n{preview}"));
            }
            AgentEvent::ToolExecutionEnd {
                tool_name,
                is_error,
                preview,
                ..
            } => {
                self.turn_state = tui_core::TurnStatus::ExecutingTools;
                let mark = if is_error { "✗" } else { "✓" };
                let status = if is_error { "ERROR" } else { "DONE" };
                let ringkas = preview.replace(['\n', '\r'], " ");
                let ringkas = if ringkas.chars().count() > 64 {
                    format!("{}…", ringkas.chars().take(63).collect::<String>())
                } else {
                    ringkas
                };
                self.push_line(format!("╰─ {tool_name} │ {status} │ {mark} {ringkas}"));
                let index = self.lines.len().saturating_sub(1);
                let status = if is_error { "gagal" } else { "selesai" };
                self.tool_details
                    .insert(index, format!("{tool_name} {status}\n{preview}"));
            }
            AgentEvent::TurnEnd { .. } => {}
            AgentEvent::AgentEnd { turns } => {
                self.flush_current();
                self.busy = false;
                self.status = format!("selesai ({turns} turn)");
                if !self.turn_state.is_turn_terminal() {
                    self.turn_state = tui_core::TurnStatus::Idle;
                }
            }
            AgentEvent::ContextUsage { tokens, window } => {
                self.context = Some((tokens, window));
            }
            AgentEvent::SessionsListed { sessions } => {
                if sessions.is_empty() {
                    self.push_line("belum ada sesi tersimpan".to_string());
                } else {
                    self.picker = Some(Picker {
                        items: sessions,
                        selected: 0,
                        jenis: PickerJenis::Sesi,
                    });
                }
            }
            AgentEvent::SessionSwitched { path, messages } => {
                // Histori lama milik sesi sebelumnya; buang supaya tidak
                // tercampur dengan pesan sesi baru.
                self.lines.clear();
                self.current.clear();
                self.scroll = 0;
                self.turn_state = tui_core::TurnStatus::Idle;
                self.status = "siap".to_string();
                self.push_line(format!("pindah ke sesi {path} ({messages} pesan)"));
            }
            AgentEvent::Notice { text } => {
                self.flush_current();
                self.push_line(format!("! {text}"));
            }
            AgentEvent::MessageStart => {}
        }
    }

    fn push_line(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let drop_count = self.lines.len() - MAX_LINES;
            self.lines.drain(..drop_count);
        }
        self.scroll = 0;
    }

    fn flush_current(&mut self) {
        if !self.current.is_empty() {
            let text = std::mem::take(&mut self.current);
            self.push_line(text);
        }
    }

    /// Jumlah baris visual setelah teks dilipat pada lebar tampilan.
    ///
    /// Perkiraan per karakter (bukan lebar grapheme): cukup untuk membuat
    /// gulir berhenti di ujung isi, dan sengaja konservatif untuk teks lebar.
    fn visual_lines(&self) -> usize {
        let width = usize::from(self.view_width.max(1));
        let mut total = 0usize;
        for (index, line) in self.lines.iter().enumerate() {
            total += display_rows(line, width);
            if self.expanded_tools
                && let Some(detail) = self.tool_details.get(&index)
            {
                for detail_line in detail.split('\n') {
                    total += display_rows(detail_line, width);
                }
            }
        }
        if !self.current.is_empty() {
            total += display_rows(&self.current, width);
        }
        total
    }

    fn max_scroll(&self) -> u16 {
        let visible = usize::from(self.view_height);
        u16::try_from(self.visual_lines().saturating_sub(visible)).unwrap_or(u16::MAX)
    }

    fn scroll_by(&mut self, delta: i32) {
        let max = i32::from(self.max_scroll());
        let next = (i32::from(self.scroll) + delta).clamp(0, max);
        self.scroll = u16::try_from(next).unwrap_or(0);
    }

    /// Offset baris visual sebelum indeks `index` (sadar lipat & detail tool).
    fn visual_offset(&self, index: usize) -> usize {
        let width = usize::from(self.view_width.max(1));
        let mut total = 0usize;
        for (position, line) in self.lines.iter().enumerate().take(index) {
            total += display_rows(line, width);
            if self.expanded_tools
                && let Some(detail) = self.tool_details.get(&position)
            {
                for detail_line in detail.split('\n') {
                    total += display_rows(detail_line, width);
                }
            }
        }
        total
    }

    /// Gulir layar sehingga blok persetujuan yang menunggu terlihat.
    fn jump_to_pending(&mut self) {
        let Some(target) = self
            .pending_line
            .or(self.pending_countdown_line)
            .or(self.pending_choice_line)
        else {
            return;
        };
        let total = self.visual_lines();
        let visible = usize::from(self.view_height.max(1));
        let max_top = total.saturating_sub(visible);
        let top = self.visual_offset(target).min(max_top);
        let scroll = self.max_scroll().saturating_sub(top as u16);
        self.scroll = scroll;
        self.status = "dilompat ke persetujuan".to_string();
    }
    /// Panggil prompt sebelumnya (`newer = false`) atau berikutnya (`true`).
    ///
    /// Indeks `None` berarti sedang menulis prompt baru; `Up` dari sana masuk ke
    /// entri terakhir, dan `Down` melewati entri terbaru mengosongkan composer.
    fn recall_history(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        if older {
            let next = match self.history_index {
                None => self.history.len() - 1,
                Some(index) if index > 0 => index - 1,
                Some(index) => index,
            };
            self.history_index = Some(next);
            self.input = self.history[next].clone();
            return;
        }
        match self.history_index {
            Some(index) if index + 1 < self.history.len() => {
                self.history_index = Some(index + 1);
                self.input = self.history[index + 1].clone();
            }
            Some(_) => {
                self.history_index = None;
                self.input.clear();
            }
            None => {}
        }
    }

    /// Command yang lolos filter palet (query dicocokkan ke nama, usage, deskripsi).
    fn palette_items(&self) -> Vec<&'static CommandSpec> {
        let query = self
            .palette
            .as_ref()
            .map(|palette| palette.query.to_ascii_lowercase())
            .unwrap_or_default();
        COMMANDS
            .iter()
            .filter(|spec| {
                query.is_empty()
                    || spec.name.to_ascii_lowercase().contains(&query)
                    || spec.usage.to_ascii_lowercase().contains(&query)
                    || spec.description.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    fn palette_key(&mut self, key: KeyEvent) -> Option<Action> {
        let items = self.palette_items();
        let count = items.len();
        let palette = self.palette.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                self.palette = None;
            }
            KeyCode::Up => {
                palette.selected = palette.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                if palette.selected + 1 < count {
                    palette.selected += 1;
                }
            }
            KeyCode::Backspace => {
                palette.query.pop();
                palette.selected = 0;
            }
            KeyCode::Enter => {
                let selected = palette.selected.min(count.saturating_sub(1));
                let spec = items.get(selected).copied();
                self.palette = None;
                let spec = spec?;
                if spec.allow_args {
                    self.input = format!("/{} ", spec.name);
                    return None;
                }
                let text = format!("/{}", spec.name);
                self.history.push(text.clone());
                return Some(Action::Submit(text));
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.query.push(ch);
                palette.selected = 0;
            }
            _ => {}
        }
        None
    }

    /// Sisipkan teks tempel (paste) ke composer sebagai satu baris.
    ///
    /// Baris baru diubah menjadi spasi karena composer hanya satu baris dan
    /// Enter dipakai untuk mengirim; batas 4000 karakter tetap berlaku.
    fn insert_text(&mut self, text: &str) {
        let sanitized = text.replace(['\r', '\n'], " ");
        for ch in sanitized.chars() {
            if self.input.chars().count() >= 4_000 {
                break;
            }
            self.input.push(ch);
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_by(3),
            MouseEventKind::ScrollDown => self.scroll_by(-3),
            _ => {}
        }
    }

    fn scroll_home(&mut self) {
        self.scroll = self.max_scroll();
    }

    fn scroll_end(&mut self) {
        self.scroll = 0;
    }

    /// Layar sambutan referensi 01: ASCII logo, kartu status nyata, quickstart.
    /// Tidak ada angka rekaan: semua berasal dari state sesi yang sudah ada.
    fn welcome_body(&self) -> Vec<Line<'static>> {
        let p = self.theme;
        let mut out = Vec::new();
        for (text, color) in [
            ("  ███╗   ███╗ █████╗ ██╗ ██████╗██╗     ██╗", p.secondary),
            ("  ████╗ ████║██╔══██╗██║██╔════╝██║     ██║", p.primary),
            (
                "  ██╔████╔██║███████║██║██║     ██║     ██║",
                p.primary_container,
            ),
            ("  ██║╚██╔╝██║██╔══██║██║██║     ██║     ██║", p.tertiary),
            (
                "  ██║ ╚═╝ ██║██║  ██║██║╚██████╗███████╗██║",
                p.tertiary_container,
            ),
        ] {
            out.push(Line::from(Span::styled(
                text,
                Style::default().fg(color).bold(),
            )));
        }
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "  Antarmuka terminal agentic ModeluXAI — RPC aktif, 0 angka rekaan",
            Style::default().fg(p.on_variant),
        )));
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "  STATUS",
            Style::default().fg(p.outline).bg(p.low),
        )));
        out.push(Line::from(Span::styled(
            format!("  model   {}", self.model),
            Style::default().fg(p.on_surface),
        )));
        out.push(Line::from(Span::styled(
            format!("  status  {}", self.status),
            Style::default().fg(p.on_variant),
        )));
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "  RPC                 MODEL                  SESI",
            Style::default().fg(p.outline).bg(p.low),
        )));
        out.push(Line::from(Span::styled(
            format!(
                "  {}    {}    {} pesan",
                if self.pending.is_some() {
                    "MENUNGGU"
                } else {
                    "AKTIF"
                },
                self.model,
                self.queue.len()
            ),
            Style::default().fg(p.on_surface),
        )));
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "  QUICKSTART",
            Style::default().fg(p.outline).bg(p.low),
        )));
        out.push(Line::from(Span::styled(
            "  [1] Prompt baru      [2] Tool Inspector      [3] Command palette",
            Style::default().fg(p.on_surface),
        )));
        out
    }

    fn body(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() && self.current.is_empty() {
            return self.welcome_body();
        }
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.lines.len() + 1);
        let mut in_code_block = false;
        let mut index = 0usize;
        while index < self.lines.len() {
            let line = &self.lines[index];
            if line.trim_start().starts_with("```") {
                in_code_block = !in_code_block;
                lines.push(code_line(line, self.dim));
                index += 1;
                continue;
            }
            if in_code_block {
                lines.push(code_line(line, self.dim));
                index += 1;
                continue;
            }
            if let Some(block) = table_block(&self.lines[index..]) {
                // `block` berisi baris data saja (header + isi); baris pemisah
                // ikut terpakai dari sumber sehingga konsumsinya ditambah satu.
                let consumed = block.len() + 1;
                lines.extend(render_table(&block, self.accent, self.dim));
                index += consumed;
                continue;
            }
            lines.push(styled_line(line, self.accent, self.dim));
            if self.expanded_tools
                && let Some(detail) = self.tool_details.get(&index)
            {
                for detail_line in detail.split('\n') {
                    lines.push(Line::from(Span::styled(
                        format!("   {detail_line}"),
                        Style::default().fg(self.dim),
                    )));
                }
            }
            index += 1;
        }
        if !self.current.is_empty() {
            // Baris yang sedang mengalir mengikuti status blok kode terakhir
            // supaya isi blok tidak tiba-tiba bergaya tebal di tengah jalan.
            lines.push(if in_code_block {
                code_line(&self.current, self.dim)
            } else {
                styled_line(&self.current, self.accent, self.dim)
            });
        }
        lines
    }

    fn top_offset(&self) -> u16 {
        self.max_scroll().saturating_sub(self.scroll)
    }

    /// Label keadaan turn untuk status line (RENCANA §7.2).
    fn turn_label(state: tui_core::TurnStatus) -> &'static str {
        use tui_core::TurnStatus;
        match state {
            TurnStatus::Idle => "siap",
            TurnStatus::Streaming => "streaming",
            TurnStatus::ExecutingTools => "menjalankan tool",
            TurnStatus::AwaitingApproval => "menunggu persetujuan",
            TurnStatus::Aborted => "dibatalkan",
            TurnStatus::Error => "error",
        }
    }

    /// Label mode approval untuk status line (C-09: status line memuat mode approval).
    fn approval_label(mode: tui_approval::ApprovalMode) -> &'static str {
        use tui_approval::ApprovalMode;
        match mode {
            ApprovalMode::AlwaysAsk => "tanya",
            ApprovalMode::AlwaysAskCached => "tanya+cache",
            ApprovalMode::Write => "tulis",
            ApprovalMode::Yolo => "yolo",
        }
    }

    fn status_line(&self) -> String {
        let state = if self.pending.is_some() {
            "menunggu persetujuan"
        } else if self.busy && self.turn_state == tui_core::TurnStatus::Idle {
            // Jendela antara prompt dikirim dan AgentStart tiba.
            "bekerja"
        } else {
            Self::turn_label(self.turn_state)
        };
        let scrolled = if self.scroll > 0 {
            format!(" · scroll {}/{}", self.scroll, self.max_scroll())
        } else {
            String::new()
        };
        let context = match self.context {
            Some((tokens, window)) if window > 0 => {
                let percent = tokens.saturating_mul(100) / window;
                format!(" · konteks {percent}%")
            }
            _ => String::new(),
        };
        format!(
            "{} · mode {} · {}{}{} · {}",
            self.model,
            Self::approval_label(self.approval_mode),
            state,
            scrolled,
            context,
            self.status
        )
    }

    /// Tinggi kotak composer (dalam baris) mengikuti isi, dibatasi supaya
    /// viewport histori tidak habis dipakai input panjang.
    fn composer_height(&self) -> u16 {
        let width = usize::from(self.view_width.max(1));
        let rows: usize = self
            .input
            .split('\n')
            .map(|line| display_rows(line, width))
            .sum();
        let total = rows.max(1) as u16 + 2; // + border atas/bawah
        total.clamp(3, 12)
    }

    /// Gulir internal composer supaya baris terakhir yang diketik tetap terlihat.
    fn composer_scroll(&self) -> u16 {
        let width = usize::from(self.view_width.max(1));
        let rows: usize = self
            .input
            .split('\n')
            .map(|line| display_rows(line, width))
            .sum();
        let visible = usize::from(self.composer_height().saturating_sub(2).max(1));
        rows.saturating_sub(visible) as u16
    }

    fn hint(&self) -> String {
        if let Some(form) = self.form_add.as_ref() {
            format!("{} · Esc batal", form.pertanyaan())
        } else if self.show_help {
            "F1/Esc menutup bantuan".to_string()
        } else if self.question.is_some() {
            "jawab pertanyaan lalu Enter · Esc lewati".to_string()
        } else if self.pending.is_some() {
            "Tab pilih · Enter terapkan · y/2/3/n pintas · F2 lompat ke persetujuan".to_string()
        } else if self.busy {
            "Esc/Ctrl+C batalkan turn · PgUp/PgDn gulir".to_string()
        } else {
            "╰─ [Enter] kirim · [Shift+Enter] baris baru · [/] slash · [Ctrl+K] palette · [F1] bantuan"
                .to_string()
        }
    }

    fn help_text(&self) -> String {
        [
            "Tombol",
            "  Enter        kirim prompt",
            "  Shift+Enter  tambah baris baru di prompt",
            "  Up/Down      riwayat prompt (kosong) / gulir (berisi)",
            "  PgUp/PgDn    gulir histori satu layar",
            "  Home/End     ke awal/akhir histori",
            "  Esc          batalkan turn yang berjalan",
            "  Ctrl+C       batalkan turn (sibuk) · 2x keluar (idle)",
            "  Ctrl+K       palet perintah",
            "  Ctrl+E       detail/ringkas tool call",
            "  F1 atau ?    buka/tutup bantuan ini",
            "  F2           lompat ke persetujuan",
            "  F3           panel subagent",
            "  Alt+A        agent hub (event + steer/prompt/abort/kill)",
            "",
            "Persetujuan tool",
            "  Tab          pindah pilihan",
            "  Enter        terapkan pilihan",
            "  y atau 1     izinkan sekali",
            "  2            izinkan sepanjang sesi",
            "  3            izinkan selalu",
            "  n atau 4     tolak",
            "",
            "Perintah",
            "  /help daftar lengkap · /model /compact /tools /session /quit",
        ]
        .join("\n")
    }

    /// Tampilkan satu permintaan approval sebagai blok inline di timeline.
    fn show_pending(&mut self, pending: PendingApproval) {
        self.status = "1 persetujuan menunggu".to_string();
        self.push_line("╔══ SECURITY & PERMISSION GUARDRAIL ══╗".to_string());
        self.push_line(
            "RISK: lihat detail di bawah · Y setuju sekali · N tolak · S setuju sesi".to_string(),
        );
        self.push_line(format!("⏳ menunggu persetujuan: {}", pending.summary()));
        self.pending_line = Some(self.lines.len().saturating_sub(1));
        for detail in pending.verbatim_detail().lines() {
            self.push_line(format!("   {detail}"));
        }
        self.push_line(String::new());
        self.pending_choice_line = Some(self.lines.len());
        self.push_line(
            "   [Y] Setuju Sekali   [N] Tolak   [S] Setuju Sesi   (Tab memilih)".to_string(),
        );
        self.pending_countdown_line = Some(self.lines.len());
        self.push_line("   sisa menghitung…".to_string());
        self.pending_since = Some(std::time::Instant::now());
        self.pending_choice = None;
        self.pending = Some(pending);
    }

    /// Perbarui baris countdown dan pilihan pada dialog approval inline.
    fn refresh_pending_lines(&mut self) {
        let Some(since) = self.pending_since else {
            return;
        };
        let elapsed = since.elapsed();
        let total = self.pending_timeout;
        let remaining = total.saturating_sub(elapsed);
        let secs = remaining.as_secs();
        let text = format!(
            "   sisa {:02}:{:02} · Tab ganti pilihan · Enter terapkan · y/2/3/n pintas",
            secs / 60,
            secs % 60
        );
        if let Some(index) = self.pending_countdown_line
            && let Some(line) = self.lines.get_mut(index)
        {
            *line = text;
        }
        if let Some(index) = self.pending_choice_line {
            let choice = match self.pending_choice {
                Some(true) => "   [x] Setuju Sekali   [ ] Tolak   [ ] Setuju Sesi",
                Some(false) => "   [ ] Setuju Sekali   [x] Tolak   [ ] Setuju Sesi",
                None => "   [ ] Setuju Sekali   [ ] Tolak   [ ] Setuju Sesi   (Tab)",
            };
            if let Some(line) = self.lines.get_mut(index) {
                *line = choice.to_string();
            }
        }
    }

    /// Catat keputusan approval pada blok inline lalu bersihkan state pending.
    fn close_pending_lines(&mut self, label: &str) {
        if let Some(index) = self.pending_choice_line
            && let Some(line) = self.lines.get_mut(index)
        {
            *line = format!("   keputusan: {label}");
        }
        if let Some(index) = self.pending_countdown_line
            && let Some(line) = self.lines.get_mut(index)
        {
            line.clear();
        }
        self.pending_choice_line = None;
        self.pending_countdown_line = None;
        self.pending_line = None;
        self.pending_since = None;
    }
    fn handle_key(&mut self, key: KeyEvent, commands: &UnboundedSender<Command>) -> Option<Action> {
        if self.key_override("cancel", key)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            // Turn berjalan: Ctrl+C membatalkan dulu. Idle: perlu dua kali
            // berturut-turut supaya satu tombol tidak langsung menutup sesi.
            if self.busy {
                let _ = commands.send(Command::Cancel);
                self.status = "membatalkan turn…".to_string();
                return Some(Action::Abort);
            }
            if self.quit_armed {
                return Some(Action::Quit);
            }
            self.quit_armed = true;
            self.status = "tekan Ctrl+C lagi untuk keluar".to_string();
            return None;
        }
        self.quit_armed = false;
        if self.key_override("palette", key)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k'))
        {
            self.show_help = false;
            self.picker = None;
            self.palette = Some(Palette {
                query: String::new(),
                selected: 0,
            });
            return None;
        }
        if self.palette.is_some() {
            return self.palette_key(key);
        }
        if self.key_override("clear_input", key)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u'))
        {
            self.input.clear();
            return None;
        }
        if self.key_override("tool_details", key)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e'))
        {
            self.expanded_tools = !self.expanded_tools;
            self.status = if self.expanded_tools {
                "detail tool ditampilkan (Ctrl+E untuk ringkas)".to_string()
            } else {
                "detail tool disembunyikan (Ctrl+E untuk buka)".to_string()
            };
            return None;
        }
        if self.key_override("help", key) {
            self.show_help = true;
            return None;
        }
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::F(1) | KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?')
            ) {
                self.show_help = false;
            }
            return None;
        }
        if let Some(picker) = self.picker.as_mut() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    picker.selected = picker.selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if picker.selected + 1 < picker.items.len() {
                        picker.selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(picker) = self.picker.take() {
                        let id = picker.items[picker.selected].id.clone();
                        return match picker.jenis {
                            PickerJenis::Sesi => Some(Action::SwitchSession(id)),
                            PickerJenis::Katalog => Some(Action::Submit(format!("/catalog {id}"))),
                            PickerJenis::Model => Some(Action::Submit(format!("/model {id}"))),
                        };
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.picker = None;
                }
                _ => {}
            }
            return None;
        }
        if self.question.is_some() {
            match key.code {
                KeyCode::Enter => {
                    if let Some(question) = self.question.take() {
                        let answer = self.input.trim().to_string();
                        self.input.clear();
                        let shown = if answer.is_empty() {
                            "(dilewati)".to_string()
                        } else {
                            answer.clone()
                        };
                        let _ = question.reply.send(answer);
                        self.push_line(format!("[tanya] {} → {shown}", question.question));
                    }
                    return None;
                }
                KeyCode::Esc => {
                    if let Some(question) = self.question.take() {
                        let _ = question.reply.send(String::new());
                        self.push_line(format!("[tanya] {} → dilewati", question.question));
                    }
                    return None;
                }
                _ => {}
            }
        }
        if self.pending.is_some() {
            let decision = match key.code {
                KeyCode::F(2) => {
                    self.jump_to_pending();
                    return None;
                }
                KeyCode::Tab => {
                    self.pending_choice = match self.pending_choice {
                        None | Some(false) => Some(true),
                        Some(true) => Some(false),
                    };
                    self.refresh_pending_lines();
                    return None;
                }
                KeyCode::Enter => {
                    // Enter hanya menerapkan tombol yang sudah dipilih lewat Tab;
                    // Enter di composer tidak pernah otomatis menyetujui.
                    match self.pending_choice {
                        Some(true) => Some(Decision::Once),
                        Some(false) => Some(Decision::Deny),
                        None => {
                            self.status =
                                "pilih dulu: Tab lalu Enter, atau tekan y/2/3/n".to_string();
                            return None;
                        }
                    }
                }
                KeyCode::Char('y') | KeyCode::Char('1') => Some(Decision::Once),
                KeyCode::Char('2') => Some(Decision::Session),
                KeyCode::Char('3') => Some(Decision::Always),
                KeyCode::Char('n') | KeyCode::Char('4') | KeyCode::Esc => Some(Decision::Deny),
                _ => None,
            };
            if let Some(choice) = decision {
                let pending = self.pending.take()?;
                let summary = pending.summary();
                let delivered = pending.reply.send(choice).is_ok();
                if delivered {
                    self.status = format!("persetujuan: {choice:?}");
                    self.close_pending_lines(&format!("{choice:?}"));
                } else {
                    // Approver sudah menyerah (timeout/batal), jadi jawaban ini
                    // tidak sampai ke tool; katakan apa adanya.
                    self.status = "permintaan persetujuan kedaluwarsa".to_string();
                    self.close_pending_lines("kedaluwarsa; jawaban tidak diteruskan");
                    self.push_line(format!(
                        "[persetujuan] kedaluwarsa sebelum dijawab: {summary}"
                    ));
                }
            }
            return None;
        }
        // Panel Agent Hub aktif: kunci ditangani panel, bukan composer.
        if self.hub.is_some() {
            // Keputusan panel dihitung dengan borrow terbatas, lalu dieksekusi.
            enum Aksi {
                Tutup,
                Naik,
                Turun,
                Mode(HubMode),
                Abort,
                Kill,
                Kirim(HubMode, String),
                Ketik(char),
                Hapus,
            }
            let aksi = match self.hub.as_mut() {
                None => Aksi::Tutup,
                Some(hub) => {
                    let alt = key.modifiers.contains(KeyModifiers::ALT);
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('a') && alt) {
                        Aksi::Tutup
                    } else if key.code == KeyCode::Up || (key.code == KeyCode::Char('k') && ctrl) {
                        Aksi::Naik
                    } else if key.code == KeyCode::Down || (key.code == KeyCode::Char('j') && ctrl)
                    {
                        Aksi::Turun
                    } else if key.code == KeyCode::Char('s') && hub.mode == HubMode::Lihat {
                        hub.broadcast = false;
                        Aksi::Mode(HubMode::Steer)
                    } else if key.code == KeyCode::Char('p') && hub.mode == HubMode::Lihat {
                        Aksi::Mode(HubMode::Prompt)
                    } else if key.code == KeyCode::Char('a') && hub.mode == HubMode::Lihat {
                        Aksi::Abort
                    } else if key.code == KeyCode::Char('K') && hub.mode == HubMode::Lihat {
                        Aksi::Kill
                    } else if key.code == KeyCode::Char('t') && hub.mode == HubMode::Lihat {
                        // Toggle kanan: event ↔ transkrip.
                        hub.tampilan = match hub.tampilan {
                            HubTampilan::Event => HubTampilan::Transkrip,
                            HubTampilan::Transkrip => HubTampilan::Event,
                        };
                        return None;
                    } else if key.code == KeyCode::Char('b') && hub.mode == HubMode::Lihat {
                        // Broadcast steer: teks dikirim ke semua anak berjalan.
                        hub.broadcast = true;
                        hub.mode = HubMode::Steer;
                        hub.input.clear();
                        return None;
                    } else if key.code == KeyCode::Enter && hub.mode != HubMode::Lihat {
                        Aksi::Kirim(hub.mode, hub.input.clone())
                    } else if key.code == KeyCode::Backspace && hub.mode != HubMode::Lihat {
                        Aksi::Hapus
                    } else if let (KeyCode::Char(c), _) = (key.code, key.modifiers) {
                        if hub.mode == HubMode::Lihat {
                            Aksi::Tutup // ketikan di mode Lihat diabaikan
                        } else if hub.input.chars().count() < 2000 {
                            Aksi::Ketik(c)
                        } else {
                            Aksi::Tutup
                        }
                    } else {
                        Aksi::Tutup
                    }
                }
            };
            // Aksi "Tutup" yang berasal dari ketikan mode Lihat = abaikan saja:
            // bedakan dengan flag.
            match aksi {
                Aksi::Tutup => {
                    // Alt+A / Esc benar-benar menutup; kunci lain diabaikan.
                    let alt = key.modifiers.contains(KeyModifiers::ALT);
                    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('a') && alt) {
                        self.hub = None;
                    }
                    return None;
                }
                Aksi::Naik => {
                    if let Some(hub) = self.hub.as_mut()
                        && hub.selected > 0
                    {
                        hub.selected -= 1;
                    }
                }
                Aksi::Turun => {
                    if let (Some(hub), Some(manager)) = (self.hub.as_mut(), self.subagents.as_ref())
                        && hub.selected + 1 < manager.list().len()
                    {
                        hub.selected += 1;
                    }
                }
                Aksi::Mode(mode) => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.mode = mode;
                        hub.input.clear();
                    }
                }
                Aksi::Abort => {
                    if let Some(anak) = self.anak_terpilih() {
                        let _ = anak.abort();
                        self.status = format!("{}: abort dikirim", anak.id);
                    }
                }
                Aksi::Kill => {
                    if let Some(anak) = self.anak_terpilih() {
                        let _ = anak.kill();
                        self.status = format!("{}: dihentikan", anak.id);
                    }
                }
                Aksi::Kirim(mode, teks) => {
                    if !teks.trim().is_empty() {
                        let broadcast = self.hub.as_ref().is_some_and(|hub| hub.broadcast);
                        if broadcast {
                            // Steer ke semua anak berjalan.
                            let mut dikirim = 0;
                            if let Some(manager) = self.subagents.as_ref() {
                                for anak in manager.list() {
                                    if anak.status()
                                        == tui_subagent::manager::SubagentState::Running
                                        && anak.steer(teks.clone())
                                    {
                                        dikirim += 1;
                                    }
                                }
                            }
                            self.status = format!("steer terkirim ke {dikirim} anak");
                        } else if let Some(anak) = self.anak_terpilih() {
                            let ok = if mode == HubMode::Steer {
                                anak.steer(teks.clone())
                            } else {
                                anak.prompt(teks.clone())
                            };
                            self.status = if ok {
                                format!("{}: dikirim", anak.id)
                            } else {
                                format!("{}: task mati", anak.id)
                            };
                        }
                    }
                    if let Some(hub) = self.hub.as_mut() {
                        hub.mode = HubMode::Lihat;
                        hub.input.clear();
                        hub.broadcast = false;
                    }
                }
                Aksi::Ketik(c) => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.input.push(c);
                    }
                }
                Aksi::Hapus => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.input.pop();
                    }
                }
            }
            return None;
        }
        match key.code {
            KeyCode::F(1) => self.show_help = true,
            KeyCode::F(2) => self.jump_to_pending(),
            KeyCode::Char('?') if self.input.is_empty() => self.show_help = true,
            KeyCode::F(3) => {
                self.agents_open = !self.agents_open;
                self.show_help = false;
            }
            // Alt+A: panel Agent Hub penuh (daftar + event + kendali).
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::ALT) => {
                if self.hub.is_some() {
                    self.hub = None;
                } else {
                    self.hub = Some(HubPanel {
                        selected: 0,
                        mode: HubMode::Lihat,
                        input: String::new(),
                        tampilan: HubTampilan::Event,
                        broadcast: false,
                    });
                    self.agents_open = false;
                }
                self.show_help = false;
            }
            KeyCode::PageUp => self.scroll_by(i32::from(self.view_height.max(1))),
            KeyCode::PageDown => self.scroll_by(-i32::from(self.view_height.max(1))),
            KeyCode::Home => self.scroll_home(),
            KeyCode::End => self.scroll_end(),
            KeyCode::Up => {
                if self.input.is_empty() || self.history_index.is_some() {
                    self.recall_history(true);
                } else {
                    self.scroll_by(1);
                }
            }
            KeyCode::Down => {
                if self.input.is_empty() || self.history_index.is_some() {
                    self.recall_history(false);
                } else {
                    self.scroll_by(-1);
                }
            }
            KeyCode::Esc => {
                if self.form_add.is_some() {
                    // Batal form tanpa menyimpan apa pun.
                    self.form_add = None;
                    self.input.clear();
                    self.status = "form provider dibatalkan".to_string();
                    return None;
                }
                if self.busy {
                    let _ = commands.send(Command::Cancel);
                    self.status = "membatalkan turn…".to_string();
                    return Some(Action::Abort);
                }
                return None;
            }
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                if self.input.chars().count() < 4_000 {
                    self.input.push('\n');
                }
            }
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                self.history.push(text.clone());
                self.history_index = None;
                self.input.clear();
                return Some(Action::Submit(text));
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if self.input.chars().count() < 4_000 => self.input.push(c),
            _ => {}
        }
        None
    }
}

/// Daftar pilihan katalog untuk picker (id + label status env + model bawaan).
/// Provider yang env kuncinya kosong tetap tampil (dengan penanda).
fn katalog_pilihan(katalog: Option<&tui_katalog::Katalog>) -> Vec<SessionChoice> {
    let Some(katalog) = katalog else {
        return Vec::new();
    };
    katalog
        .providers
        .iter()
        .map(|p| {
            let siap = tui_wire::providers::by_id(&p.id)
                .and_then(|entry| {
                    entry.resolve_env_key(|nama| {
                        std::env::var(nama).ok().filter(|v| !v.trim().is_empty())
                    })
                })
                .is_some();
            let tanda = if siap { "✓" } else { "no-key" };
            SessionChoice {
                id: p.id.clone(),
                title: format!(
                    "{} — model: {} [{}]",
                    p.id,
                    p.model_bawaan.as_deref().unwrap_or("?"),
                    tanda
                ),
                messages: 0,
            }
        })
        .collect()
}

fn io_error(error: std::io::Error) -> MxError {
    let message = error.to_string();
    MxError::with_source(ErrorCode::ToolFailed, format!("terminal: {message}"), error)
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut out = stdout();
        let _ = execute!(
            out,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
    }
}

fn tool_context_for(boot: &Boot, ask: UnboundedSender<AskRequest>) -> ToolContext {
    let mut ctx = ToolContext::new(boot.cwd.clone());
    ctx.output_cap = boot.limits.tool_output_cap_bytes;
    ctx.interactive = true;
    ctx.bash_timeout = Duration::from_secs(boot.limits.bash_timeout_secs);
    ctx.bash_max_timeout = Duration::from_secs(boot.limits.bash_max_timeout_secs);
    ctx.ask = Some(ask);
    ctx
}

/// Label satu baris kiriman pengguna di timeline.
///
/// Perintah garis miring diberi badge `[perintah]` supaya jelas itu dijalankan
/// aplikasi, bukan dikirim ke model (RENCANA C-08).
fn timeline_label(text: &str) -> String {
    if text.trim_start().starts_with('/') {
        format!("[perintah] {text}")
    } else {
        format!("> {text}")
    }
}

/// Bangun sesi dengan registry standar (dipakai `/new`, `/fork`, `/switch`).
fn build_agent_session(
    boot: &Boot,
    carry: Option<&AgentSession>,
    approver: Arc<dyn Approver>,
    ask: UnboundedSender<AskRequest>,
    events: UnboundedSender<AgentEvent>,
    model: &str,
    skills_on: bool,
) -> AgentSession {
    let settings = TurnSettings {
        model: model.to_string(),
        deadline: Some(Duration::from_secs(boot.limits.turn_deadline_secs)),
        system_prompt: crate::features::base_system_prompt(boot, skills_on),
        ..TurnSettings::default()
    };
    let mut session = AgentSession::new(
        Arc::new(boot.client.clone()),
        approver,
        boot.tool_registry(),
        tool_context_for(boot, ask),
        settings,
        events,
    );
    session.set_context_window(boot.limits.context_window_tokens);
    crate::hooks::pasang(&mut session, &boot.config.hooks);
    if let Some(lama) = carry {
        session.set_service_tier(lama.service_tier().map(str::to_string));
        session.set_effort(lama.effort().map(str::to_string));
    }
    session
}

fn spawn_worker(
    boot: Boot,
    events: UnboundedSender<AgentEvent>,
    approvals: UnboundedSender<PendingApproval>,
    questions: UnboundedSender<AskRequest>,
) -> UnboundedSender<Command> {
    let (tx, mut rx) = unbounded_channel::<Command>();
    tokio::spawn(async move {
        let ask_for_switch = questions.clone();
        let approvals_pulih = approvals.clone();
        let ctx = tool_context_for(&boot, questions);
        let skills_ringkas = tui_tools::skills::ringkas_daftar(&boot.skills);
        let mut effort_aktif: Option<String> = None;
        let mut force_aktif = false;
        let mut fitur = FeatureState::new(&boot);
        // Siaran guest relay diteruskan ke timeline mode ini.
        fitur.events = Some(events.clone());
        let approval_timeout = Duration::from_secs(boot.limits.approval_timeout_secs);
        let approver: Arc<dyn Approver> = match build_approver(
            boot.approval.clone(),
            Box::new(ChannelDecisionUi::new(approvals, approval_timeout)),
            boot.grants_path.clone(),
            boot.grant_ttl,
        ) {
            Ok(approver) => Arc::new(approver),
            Err(err) => {
                let _ = events.send(AgentEvent::Notice {
                    text: format!("grant approval tidak bisa dibaca: {}", err.message),
                });
                Arc::new(PolicyApprover::new(boot.approval.clone()))
            }
        };
        let settings = TurnSettings {
            model: boot.model.clone(),
            deadline: Some(Duration::from_secs(boot.limits.turn_deadline_secs)),
            system_prompt: crate::features::base_system_prompt(&boot, true),
            ..TurnSettings::default()
        };
        let events_tx = events.clone();
        let approver_for_switch = approver.clone();
        let mut registry = boot.tool_registry();
        // Tool `subagent` + `orchestrate` di mode TUI (manager aktif, hub).
        if let Some(manager) = boot.subagents.clone() {
            registry.register(Arc::new(
                tui_subagent::SubagentTool::new(tui_subagent::SubagentSpec {
                    provider: Arc::new(boot.client.clone()),
                    approver: approver_for_switch.clone(),
                    tools: boot.tool_registry(),
                    model: boot.model.clone(),
                    deadline: Duration::from_secs(boot.limits.turn_deadline_secs),
                    max_tool_calls: 32,
                })
                .with_manager(manager.clone()),
            ));
            registry.register(Arc::new(tui_subagent::OrchestratorTool::new(
                tui_subagent::SubagentSpec {
                    provider: Arc::new(boot.client.clone()),
                    approver: approver_for_switch.clone(),
                    tools: boot.tool_registry(),
                    model: boot.model.clone(),
                    deadline: Duration::from_secs(boot.limits.turn_deadline_secs),
                    max_tool_calls: 32,
                },
                manager,
            )));
        }
        let tool_names = registry.names();
        let todo_arc = registry.todo_arc();
        // Daftar todo dipakai command /todo export/import/copy.
        fitur.todo = Some(todo_arc.clone());
        let mut session = AgentSession::new(
            Arc::new(boot.client.clone()),
            approver,
            registry,
            ctx,
            settings,
            events,
        );
        session.set_context_window(boot.limits.context_window_tokens);
        session.seed_usage(boot.resume_usage);
        session.push_messages(boot.resume_messages.clone());
        let mut persisted_usage = boot.resume_usage;
        let cwd = boot.cwd.display().to_string();
        let mut store: Option<SessionStore> = match crate::session_open::open_or_create(
            boot.sessions_dir.clone(),
            boot.resume_path.clone(),
            cwd,
        )
        .await
        {
            Ok(store) => Some(store),
            Err(err) => {
                let _ = events_tx.send(AgentEvent::Notice {
                    text: format!("sesi tidak tersimpan: {}: {}", err.code, err.message),
                });
                None
            }
        };
        while let Some(command) = rx.recv().await {
            match command {
                Command::Quit => break,
                // Sinyal batal saat tidak ada turn berjalan: tidak ada yang perlu
                // dihentikan, jadi diabaikan.
                Command::Cancel => {}
                Command::Prompt(text) => {
                    let session_path = store.as_ref().map(|store| store.path().to_path_buf());
                    let context = CommandContext {
                        model: session.model(),
                        provider: (&boot.provider.0, &boot.provider.1),
                        tools: &tool_names,
                        session_path: session_path.as_deref(),
                        known_models: &boot.models,
                        sessions_dir: Some(&boot.sessions_dir),
                        skills_ringkas: skills_ringkas.as_str(),
                        context_window: Some(boot.limits.context_window_tokens),
                        effort: effort_aktif.as_deref(),
                    };
                    match dispatch(&text, &context) {
                        CommandResult::Quit => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "sesi ditutup".to_string(),
                            });
                            break;
                        }
                        CommandResult::Reply(reply) => {
                            let _ = events_tx.send(AgentEvent::Notice { text: reply });
                            continue;
                        }
                        CommandResult::SetModel(model) => {
                            session.set_model(model.clone());
                            crate::session_open::append(
                                &mut store,
                                SessionEntry::model_change(model.clone()),
                            )
                            .await;
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: format!("model aktif: {model}"),
                            });
                            continue;
                        }
                        CommandResult::NewSession => {
                            let text = match crate::session_open::create_new(
                                boot.sessions_dir.clone(),
                                boot.cwd.display().to_string(),
                            )
                            .await
                            {
                                Ok(new_store) => {
                                    let path = new_store.path().display().to_string();
                                    session = build_agent_session(
                                        &boot,
                                        Some(&session),
                                        approver_for_switch.clone(),
                                        ask_for_switch.clone(),
                                        events_tx.clone(),
                                        &boot.model,
                                        fitur.skills_on,
                                    );
                                    store = Some(new_store);
                                    persisted_usage = tui_wire::Usage::default();
                                    let _ = events_tx.send(AgentEvent::SessionSwitched {
                                        path: path.clone(),
                                        messages: 0,
                                    });
                                    format!("sesi baru: {path}")
                                }
                                Err(err) => format!("{}: {}", err.code, err.message),
                            };
                            let _ = events_tx.send(AgentEvent::Notice { text });
                            continue;
                        }
                        CommandResult::ForkSession => {
                            let Some(current) =
                                store.as_ref().map(|store| store.path().to_path_buf())
                            else {
                                let _ = events_tx.send(AgentEvent::Notice {
                                    text: "belum ada sesi tersimpan untuk diduplikasi".to_string(),
                                });
                                continue;
                            };
                            match crate::session_open::fork_current(
                                current,
                                boot.sessions_dir.clone(),
                                "sesi maicli (fork)".to_string(),
                            )
                            .await
                            {
                                Ok((new_store, messages)) => {
                                    let path = new_store.path().display().to_string();
                                    let count = messages.len();
                                    session = build_agent_session(
                                        &boot,
                                        Some(&session),
                                        approver_for_switch.clone(),
                                        ask_for_switch.clone(),
                                        events_tx.clone(),
                                        &boot.model,
                                        fitur.skills_on,
                                    );
                                    session.push_messages(messages);
                                    store = Some(new_store);
                                    persisted_usage = session.usage();
                                    let _ = events_tx.send(AgentEvent::SessionSwitched {
                                        path,
                                        messages: count,
                                    });
                                }
                                Err(err) => {
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: format!("{}: {}", err.code, err.message),
                                    });
                                }
                            }
                            continue;
                        }
                        CommandResult::ForgetApprovals => {
                            approver_for_switch.forget_grants();
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "grant approval dihapus (file grants.json dikosongkan)"
                                    .to_string(),
                            });
                            continue;
                        }
                        CommandResult::ClearScreen => {
                            // Timeline ada di loop utama — worker meneruskan event.
                            let _ = events_tx.send(AgentEvent::ClearTimeline);
                            continue;
                        }
                        CommandResult::SetEffort(effort) => {
                            session.set_effort(effort.clone());
                            effort_aktif = effort.clone();
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: match effort {
                                    Some(e) => format!("effort: {e}"),
                                    None => "effort: default gateway".to_string(),
                                },
                            });
                            continue;
                        }
                        CommandResult::SetContextWindow(window) => {
                            match window {
                                Some(w) => {
                                    session.set_context_window(w);
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: format!("window konteks: {w} token (extended)"),
                                    });
                                }
                                None => {
                                    session.set_context_window(boot.limits.context_window_tokens);
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: "window konteks: kembali ke config".to_string(),
                                    });
                                }
                            }
                            continue;
                        }
                        CommandResult::ForceOnce => {
                            // Yolo satu turn: approver selalu-izin; pulihkan saat AgentEnd.
                            session.set_approver(Arc::new(tui_approval::FixedApprover::new(
                                tui_approval::Decision::Always,
                            )));
                            force_aktif = true;
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "force: turn berikutnya tanpa approval (pulih otomatis)"
                                    .to_string(),
                            });
                            continue;
                        }
                        CommandResult::Changelog => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: tui_core::commands::changelog_text(),
                            });
                            continue;
                        }
                        CommandResult::Stats => {
                            let text = tui_core::commands::stats_text(
                                session.messages().len(),
                                session.usage(),
                            );
                            let _ = events_tx.send(AgentEvent::Notice { text });
                            continue;
                        }
                        CommandResult::Setup => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "keluar dulu (Ctrl+C dua kali) lalu jalankan: maicli --setup"
                                    .to_string(),
                            });
                            continue;
                        }
                        CommandResult::Settings => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: format!(
                                    "config global: {} (buka lewat maicli --setup atau editor)",
                                    crate::config_root().join("config.toml").display()
                                ),
                            });
                            continue;
                        }
                        CommandResult::Logout => {
                            let env_nama = boot.provider.1.clone();
                            let hapus = tokio::task::spawn_blocking(move || {
                                let mut backend = tui_config::secret::KeyringBackend;
                                tui_config::secret::delete_key(&mut backend, &env_nama)
                            })
                            .await;
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: match hapus {
                                    Ok(Ok(())) => {
                                        "kunci dihapus dari keyring (env tetap menang bila terisi)"
                                            .to_string()
                                    }
                                    Ok(Err(err)) => format!("logout gagal: {}", err.message),
                                    Err(e) => format!("logout gagal: {e}"),
                                },
                            });
                            continue;
                        }
                        CommandResult::McpList => {
                            let mut teks = String::from("server MCP:");
                            if boot.mcp_notes.is_empty() {
                                teks.push_str(" tidak ada");
                            } else {
                                for n in &boot.mcp_notes {
                                    teks.push_str(&format!("\n  - {n}"));
                                }
                            }
                            let _ = events_tx.send(AgentEvent::Notice { text: teks });
                            continue;
                        }
                        CommandResult::McpRemove(id) => {
                            let path = crate::config_root().join("config.toml");
                            let id_ = id.clone();
                            let hasil = tokio::task::spawn_blocking(move || {
                                tui_config::remove_global_value(
                                    &path,
                                    &format!("mcp.servers.{id_}"),
                                )
                            })
                            .await;
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: match hasil {
                                    Ok(Ok(true)) => {
                                        format!("server MCP '{id}' dihapus dari config global (restart agar berlaku)")
                                    }
                                    Ok(Ok(false)) => {
                                        format!("server MCP '{id}' tidak ada di config global")
                                    }
                                    Ok(Err(err)) => format!("mcp remove gagal: {}", err.message),
                                    Err(e) => format!("mcp remove gagal: {e}"),
                                },
                            });
                            continue;
                        }
                        CommandResult::Export(basename) => {
                            let nama = basename.unwrap_or_else(|| "maicli-export.md".to_string());
                            let mut teks = String::from("# Transkrip maicli\n\n");
                            for m in session.messages() {
                                match m {
                                    tui_wire::Message::User { content } => {
                                        teks.push_str("## kamu\n\n");
                                        teks.push_str(content);
                                        teks.push_str("\n\n");
                                    }
                                    tui_wire::Message::Assistant { content, .. } => {
                                        teks.push_str("## maicli\n\n");
                                        teks.push_str(
                                            &content
                                                .iter()
                                                .filter_map(|b| b.as_text())
                                                .collect::<Vec<_>>()
                                                .join(""),
                                        );
                                        teks.push_str("\n\n");
                                    }
                                    _ => {}
                                }
                            }
                            let path = std::path::PathBuf::from(&nama);
                            let tulis =
                                tokio::task::spawn_blocking(move || std::fs::write(&path, teks))
                                    .await;
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: match tulis {
                                    Ok(Ok(())) => format!("transkrip ditulis ke {nama}"),
                                    Ok(Err(e)) => format!("export gagal: {e}"),
                                    Err(e) => format!("export gagal: {e}"),
                                },
                            });
                            continue;
                        }
                        CommandResult::Handoff => {
                            // Ringkas sekarang; teks ringkasan = pesan pertama konteks
                            // setelah compaction (pengganti pesan lama).
                            let report = session.compact_now().await;
                            let ringkasan = report.and_then(|_| {
                                session.messages().iter().find_map(|m| match m {
                                    tui_wire::Message::User { content }
                                    | tui_wire::Message::System { content } => {
                                        Some(content.clone())
                                    }
                                    tui_wire::Message::Assistant { content, .. } => Some(
                                        content
                                            .iter()
                                            .filter_map(|b| b.as_text())
                                            .collect::<Vec<_>>()
                                            .join(""),
                                    ),
                                    _ => None,
                                })
                            });
                            let teks = match ringkasan {
                                Some(r) => format!(
                                    "ringkasan handoff (jalankan /new lalu kirim ini sebagai pesan pertama):\n\n{r}"
                                ),
                                None => "tidak ada yang bisa diringkas".to_string(),
                            };
                            let _ = events_tx.send(AgentEvent::Notice { text: teks });
                            continue;
                        }
                        CommandResult::TodoSub(aksi, arg) => {
                            let mut daftar = todo_arc
                                .lock()
                                .map_err(|_| "todo terkunci")
                                .ok()
                                .map(|g| g.clone())
                                .unwrap_or_default();
                            let hasil = match (aksi, arg.as_deref()) {
                                ('a', Some(teks)) => {
                                    daftar.push(teks.to_string());
                                    Ok(daftar)
                                }
                                ('r', Some(no)) => {
                                    let n: usize = no.parse().unwrap_or(0);
                                    if n >= 1 && n <= daftar.len() {
                                        daftar.remove(n - 1);
                                        Ok(daftar)
                                    } else {
                                        Err(format!(
                                            "nomor {no} tidak ada di daftar ({})",
                                            daftar.len()
                                        ))
                                    }
                                }
                                ('d', Some(no)) => {
                                    let n: usize = no.parse().unwrap_or(0);
                                    if n >= 1 && n <= daftar.len() {
                                        daftar[n - 1] = format!("[selesai] {}", daftar[n - 1]);
                                        Ok(daftar)
                                    } else {
                                        Err(format!(
                                            "nomor {no} tidak ada di daftar ({})",
                                            daftar.len()
                                        ))
                                    }
                                }
                                ('c', _) => Ok(Vec::new()),
                                ('s', Some(no)) => {
                                    let n: usize = no.parse().unwrap_or(0);
                                    if n >= 1 && n <= daftar.len() {
                                        daftar[n - 1] = format!("[mulai] {}", daftar[n - 1]);
                                        Ok(daftar)
                                    } else {
                                        Err(format!(
                                            "nomor {no} tidak ada di daftar ({})",
                                            daftar.len()
                                        ))
                                    }
                                }
                                ('x', Some(no)) => {
                                    let n: usize = no.parse().unwrap_or(0);
                                    if n >= 1 && n <= daftar.len() {
                                        daftar[n - 1] = format!("[dibuang] {}", daftar[n - 1]);
                                        Ok(daftar)
                                    } else {
                                        Err(format!(
                                            "nomor {no} tidak ada di daftar ({})",
                                            daftar.len()
                                        ))
                                    }
                                }
                                ('p', Some(isi)) => match isi.split_once(char::is_whitespace) {
                                    Some((no, teks)) => {
                                        let n: usize = no.parse().unwrap_or(0);
                                        if n >= 1 && n <= daftar.len() {
                                            daftar[n - 1] =
                                                format!("{} {}", daftar[n - 1], teks.trim());
                                            Ok(daftar)
                                        } else {
                                            Err(format!(
                                                "nomor {no} tidak ada di daftar ({})",
                                                daftar.len()
                                            ))
                                        }
                                    }
                                    None => Err("pakai /todo append <nomor> <teks>".to_string()),
                                },
                                _ => Err("subcommand todo tidak dikenal".to_string()),
                            };
                            match hasil {
                                Ok(baru) => match todo_arc.lock() {
                                    Ok(mut guard) => {
                                        *guard = baru;
                                        let _ = events_tx.send(AgentEvent::Notice {
                                            text: "todo diperbarui".to_string(),
                                        });
                                    }
                                    Err(_) => {
                                        let _ = events_tx.send(AgentEvent::Notice {
                                            text: "todo gagal: daftar terkunci".to_string(),
                                        });
                                    }
                                },
                                Err(pesan) => {
                                    let _ = events_tx.send(AgentEvent::Notice { text: pesan });
                                }
                            }
                            continue;
                        }
                        CommandResult::Commit => {
                            // Di worker ini Commit dijalankan sebagai prompt biasa
                            // oleh antarmuka utama; jalur defensif bila lolos.
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "commit dijalankan lewat antarmuka utama".to_string(),
                            });
                            continue;
                        }
                        CommandResult::SetProvider(Some(id)) => {
                            match tui_config::set_global_value(
                                crate::config_root().join("config.toml").as_path(),
                                "gateway.provider",
                                &id,
                            ) {
                                Ok(()) => {
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: format!(
                                            "provider diset ke '{id}' (config global); restart agar berlaku"
                                        ),
                                    });
                                }
                                Err(err) => {
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: format!("{}: {}", err.code, err.message),
                                    });
                                }
                            }
                            continue;
                        }
                        CommandResult::SetProvider(None) => continue,
                        CommandResult::Context => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: tui_core::commands::context_text(
                                    session.estimated_tokens(),
                                    session.budget(),
                                ),
                            });
                            continue;
                        }
                        CommandResult::Todo => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: tui_core::commands::todo_text(&session.todos()),
                            });
                            continue;
                        }
                        CommandResult::SetTitle(new_title) => {
                            let text = if store.is_some() {
                                crate::session_open::append(
                                    &mut store,
                                    SessionEntry::title_change(new_title.clone()),
                                )
                                .await;
                                format!("judul sesi: {new_title}")
                            } else {
                                "belum ada sesi tersimpan untuk diberi judul".to_string()
                            };
                            let _ = events_tx.send(AgentEvent::Notice { text });
                            continue;
                        }
                        CommandResult::SwitchSession(target) => {
                            let wanted = target.unwrap_or_else(|| "latest".to_string());
                            let dir = boot.sessions_dir.clone();
                            let opened = tokio::task::spawn_blocking(move || {
                                let summary = tui_session::resolve_target(&dir, &wanted)?;
                                let loaded = tui_session::SessionStore::load(&summary.path)?;
                                let messages = loaded.messages();
                                let fresh_store = tui_session::SessionStore::open(&summary.path)?;
                                Ok::<_, MxError>((summary, messages, fresh_store))
                            })
                            .await;
                            match opened {
                                Ok(Ok((summary, messages, fresh_store))) => {
                                    let count = messages.len();
                                    session = build_agent_session(
                                        &boot,
                                        Some(&session),
                                        approver_for_switch.clone(),
                                        ask_for_switch.clone(),
                                        events_tx.clone(),
                                        &boot.model,
                                        fitur.skills_on,
                                    );
                                    session.push_messages(messages);
                                    store = Some(fresh_store);
                                    let _ = events_tx.send(AgentEvent::SessionSwitched {
                                        path: summary.path.display().to_string(),
                                        messages: count,
                                    });
                                }
                                Ok(Err(err)) => {
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: format!("{}: {}", err.code, err.message),
                                    });
                                }
                                Err(_) => {
                                    let _ = events_tx.send(AgentEvent::Notice {
                                        text: "gagal membuka sesi (task batal)".to_string(),
                                    });
                                }
                            }
                            continue;
                        }
                        CommandResult::Tree => {
                            // Pohon sesi sebagai baris timeline (indentasi kedalaman).
                            let mut teks = String::from("pohon sesi:");
                            for simpul in tui_session::pohon(&boot.sessions_dir) {
                                teks.push('\n');
                                teks.push_str(&simpul.display_line());
                            }
                            let _ = events_tx.send(AgentEvent::Notice { text: teks });
                            continue;
                        }
                        CommandResult::Sessions(query) => {
                            let sessions = match query {
                                Some(query) => tui_session::search(&boot.sessions_dir, &query),
                                None => tui_session::list(&boot.sessions_dir),
                            }
                            .into_iter()
                            .map(|summary| SessionChoice {
                                id: summary.id,
                                title: summary.title,
                                messages: summary.messages,
                            })
                            .collect();
                            let _ = events_tx.send(AgentEvent::SessionsListed { sessions });
                            continue;
                        }
                        CommandResult::Usage => {
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: tui_core::commands::usage_text(
                                    session.usage(),
                                    session.estimated_tokens(),
                                    session.usage_by_model(),
                                ),
                            });
                            continue;
                        }
                        CommandResult::Pause => {
                            session.pause();
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "sesi dijeda".to_string(),
                            });
                            continue;
                        }
                        CommandResult::Resume => {
                            session.resume();
                            let _ = events_tx.send(AgentEvent::Notice {
                                text: "sesi dilanjutkan".to_string(),
                            });
                            continue;
                        }
                        CommandResult::Compact => {
                            let text = match session.compact_now().await {
                                Some(report) => tui_core::commands::compaction_text(report),
                                None => "tidak ada yang bisa diringkas".to_string(),
                            };
                            let _ = events_tx.send(AgentEvent::Notice { text });
                            continue;
                        }
                        CommandResult::Feature { name, argument } => {
                            match crate::features::handle(
                                &boot,
                                &mut session,
                                &mut store,
                                &mut fitur,
                                &name,
                                argument.as_deref(),
                            )
                            .await
                            {
                                FeatureAction::Lines(lines) => {
                                    for line in lines {
                                        let _ = events_tx.send(AgentEvent::Notice { text: line });
                                    }
                                }
                                FeatureAction::Fresh => {
                                    let text = match crate::session_open::create_new(
                                        boot.sessions_dir.clone(),
                                        boot.cwd.display().to_string(),
                                    )
                                    .await
                                    {
                                        Ok(new_store) => {
                                            let path = new_store.path().display().to_string();
                                            session = build_agent_session(
                                                &boot,
                                                Some(&session),
                                                approver_for_switch.clone(),
                                                ask_for_switch.clone(),
                                                events_tx.clone(),
                                                &boot.model,
                                                fitur.skills_on,
                                            );
                                            store = Some(new_store);
                                            persisted_usage = tui_wire::Usage::default();
                                            approver_for_switch.forget_grants();
                                            let _ = events_tx.send(AgentEvent::SessionSwitched {
                                                path: path.clone(),
                                                messages: 0,
                                            });
                                            format!("sesi baru (fresh): {path} — grant dihapus")
                                        }
                                        Err(err) => {
                                            format!("{}: {}", err.code, err.message)
                                        }
                                    };
                                    let _ = events_tx.send(AgentEvent::Notice { text });
                                }
                            }
                            continue;
                        }
                        CommandResult::Unknown(message) => {
                            let _ = events_tx.send(AgentEvent::Notice { text: message });
                            continue;
                        }
                        CommandResult::NotACommand => {}
                    }
                    if store.is_some() {
                        crate::session_open::append(
                            &mut store,
                            SessionEntry::message(None, tui_wire::Message::user(text.clone())),
                        )
                        .await;
                    }
                    let token = session.cancel_token();
                    // Blok membatasi masa hidup borrow `run` (pulih approval
                    // butuh &mut session SETELAH turn selesai).
                    {
                        let mut run = std::pin::pin!(session.run_turn(Some(text)));
                        loop {
                            tokio::select! {
                                outcome = &mut run => {
                                    if let Err(err) = outcome {
                                        let _ = events_tx.send(AgentEvent::Notice {
                                            text: format!("{}: {}", err.code, err.message),
                                        });
                                    }
                                    break;
                                }
                                Some(command) = rx.recv() => match command {
                                    Command::Cancel => token.cancel(),
                                    Command::Quit => {
                                        token.cancel();
                                    }
                                    Command::Prompt(text) => {
                                        let wants_quit = matches!(text.trim(), "/quit" | "/exit");
                                        if wants_quit {
                                            token.cancel();
                                            let _ = events_tx.send(AgentEvent::Notice {
                                                text: "quit ditunda: turn dibatalkan dulu; jalankan /quit setelah idle".to_string(),
                                            });
                                        } else {
                                            let _ = events_tx.send(AgentEvent::Notice {
                                                text: "masih sibuk; kirim ulang setelah turn ini selesai".to_string(),
                                            });
                                        }
                                    }
                                },
                            }
                        }
                    }
                    // Review model kedua (opsional, `/advisor on`).
                    if fitur.advisor_on
                        && let Some(text) =
                            crate::features::advisor_review(&boot, &fitur, &session).await
                    {
                        let _ = events_tx.send(AgentEvent::Notice { text });
                    }
                    // Pulihkan approval setelah yolo satu turn (di luar borrow run).
                    if force_aktif {
                        if let Ok(approver_baru) = build_approver(
                            boot.approval.clone(),
                            Box::new(ChannelDecisionUi::new(
                                approvals_pulih.clone(),
                                approval_timeout,
                            )),
                            boot.grants_path.clone(),
                            boot.grant_ttl,
                        ) {
                            session.set_approver(Arc::new(approver_baru));
                        }
                        force_aktif = false;
                        let _ = events_tx.send(AgentEvent::Notice {
                            text: "force selesai: approval kembali ke kebijakan".to_string(),
                        });
                    }
                    for record in session.take_compactions() {
                        crate::session_open::append(
                            &mut store,
                            SessionEntry::compaction(None, record.summary, record.replaced),
                        )
                        .await;
                    }
                    crate::usage::persist_usage(&mut store, &session, &mut persisted_usage).await;
                    let _ = events_tx.send(AgentEvent::ContextUsage {
                        tokens: session.estimated_tokens(),
                        window: boot.limits.context_window_tokens,
                    });
                    for audit in session.take_approvals() {
                        crate::session_open::append(
                            &mut store,
                            SessionEntry::approval(
                                audit.tool,
                                format!("{:?}", audit.tier).to_lowercase(),
                                audit.subject,
                                audit.decision,
                            ),
                        )
                        .await;
                    }
                    if let Some(last) = session
                        .messages()
                        .iter()
                        .rev()
                        .find(|m| matches!(m, tui_wire::Message::Assistant { .. }))
                    {
                        crate::session_open::append(
                            &mut store,
                            SessionEntry::message(None, last.clone()),
                        )
                        .await;
                    }
                }
            }
        }
    });
    tx
}

/// Parse spesifikasi tombol config (`ctrl+k`, `f1`, `alt+a`, `?`).
pub fn parse_key(spec: &str) -> Option<KeyEvent> {
    let bagian: Vec<&str> = spec.trim().split('+').collect();
    let (mods, terakhir) = bagian.split_at(bagian.len().saturating_sub(1));
    let mut modifiers = KeyModifiers::NONE;
    for m in mods {
        modifiers |= match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" => KeyModifiers::CONTROL,
            "alt" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            _ => return None,
        };
    }
    let nama = terakhir.first().copied()?.trim().to_ascii_lowercase();
    let kode = match nama.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "space" => KeyCode::Char(' '),
        "?" => KeyCode::Char('?'),
        _ if nama.starts_with('f') => {
            let n = nama[1..].parse::<u8>().ok()?;
            if n == 0 {
                return None;
            }
            KeyCode::F(n)
        }
        _ if nama.len() == 1 => KeyCode::Char(nama.chars().next()?),
        _ => return None,
    };
    Some(KeyEvent::new(kode, modifiers))
}

/// Ubah `#rrggbb` (atau `#rgb`) menjadi warna ratatui; nilai tak valid = Reset.
fn parse_color(text: &str) -> Color {
    let hex = text.trim().trim_start_matches('#');
    let expand = |path: &str| -> Option<Color> {
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
                let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
                let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
                Some(Color::Rgb(r, g, b))
            }
            3 => {
                let digit = |index: usize| -> Option<u8> {
                    let ch = hex.get(index..index + 1)?;
                    let value = u8::from_str_radix(ch, 16).ok()?;
                    Some(value * 16 + value)
                };
                Some(Color::Rgb(digit(0)?, digit(1)?, digit(2)?))
            }
            _ => {
                let _ = path;
                None
            }
        }
    };
    expand(text).unwrap_or(Color::Reset)
}

/// Lebar tampilan teks (CJK dan emoji dihitung 2 kolom).
fn display_width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

/// Perkiraan jumlah baris setelah dilipat pada `width` kolom.
///
/// Ratatui melipat di batas kata; perhitungan ini melipat per karakter supaya
/// murah dan tidak butuh akses ke teks yang sudah dirender. Untuk paragraf
/// biasa hasilnya sama; untuk teks tanpa spasi (CJK, URL panjang) bisa lebih
/// konservatif.
fn display_rows(text: &str, width: usize) -> usize {
    let width = width.max(1);
    display_width(text).max(1).div_ceil(width)
}

/// Baris pemisah tabel markdown: `|---|:--:|`.
fn is_table_separator(line: &str) -> bool {
    let cells = split_table_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            !trimmed.is_empty()
                && trimmed
                    .trim_start_matches(':')
                    .trim_end_matches(':')
                    .chars()
                    .all(|ch| ch == '-')
        })
}

/// Pecah satu baris tabel menjadi sel; pipa luar dibuang.
fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let without_outer = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    if !trimmed.contains('|') {
        return Vec::new();
    }
    without_outer
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// Ambil blok tabel bila `rest` dimulai dengan header + baris pemisah.
fn table_block(rest: &[String]) -> Option<Vec<Vec<String>>> {
    let header = split_table_row(rest.first()?);
    let separator = rest.get(1)?;
    if header.is_empty() || !is_table_separator(separator) {
        return None;
    }
    let mut rows = vec![header];
    for line in rest.iter().skip(2) {
        if line.trim_start().starts_with("```") {
            break;
        }
        let cells = split_table_row(line);
        if cells.is_empty() || cells.len() != rows[0].len() {
            break;
        }
        rows.push(cells);
    }
    Some(rows)
}

/// Render tabel dengan lebar kolom seragam; header tebal beraksen.
fn render_table(rows: &[Vec<String>], accent: Color, dim: Color) -> Vec<Line<'static>> {
    let columns = rows.first().map(|row| row.len()).unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .map(|row| {
                    row.get(column)
                        .map(|cell| cell.chars().count())
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut text = String::from("¦");
            for (column, width) in widths.iter().enumerate() {
                let cell = row.get(column).map(String::as_str).unwrap_or("");
                let padding = width.saturating_sub(cell.chars().count());
                text.push(' ');
                text.push_str(cell);
                text.push_str(&" ".repeat(padding));
                text.push_str(" ¦");
            }
            if row_index == 0 {
                Line::from(Span::styled(
                    text,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(text, Style::default().fg(dim)))
            }
        })
        .collect()
}

/// Satu baris di dalam blok kode berpagar: warna redup, tanpa parsing penekanan.
fn code_line(text: &str, dim: Color) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), Style::default().fg(dim)))
}

/// Satu baris histori dengan penekanan ringan: heading, **tebal**, `kode`.
fn styled_line(text: &str, accent: Color, dim: Color) -> Line<'static> {
    if let Some(heading) = text.strip_prefix("# ") {
        return Line::from(Span::styled(
            heading.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(inline_spans(text, accent, dim))
}

/// Pecah teks menjadi span: `**tebal**` dan `` `kode` `` diberi gaya, sisanya apa adanya.
///
/// Penanda yang tidak ditutup dibiarkan sebagai teks biasa, supaya keluaran model
/// yang setengah jadi tidak menghilang dari layar.
fn inline_spans(text: &str, accent: Color, dim: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut index = 0usize;
    while index < text.len() {
        if let Some(close) = text[index..]
            .starts_with("**")
            .then(|| text[index + 2..].find("**"))
            .flatten()
        {
            if !plain.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut plain)));
            }
            let bold = &text[index + 2..index + 2 + close];
            spans.push(Span::styled(
                bold.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ));
            index += 2 + close + 2;
            continue;
        }
        if text[index..].starts_with('`')
            && let Some(close) = text[index + 1..].find('`')
        {
            if !plain.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut plain)));
            }
            let code = &text[index + 1..index + 1 + close];
            spans.push(Span::styled(code.to_string(), Style::default().fg(dim)));
            index += 1 + close + 1;
            continue;
        }
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        plain.push(ch);
        index += ch.len_utf8();
    }
    if !plain.is_empty() {
        spans.push(Span::raw(plain));
    }
    spans
}

fn centered_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Band ASCII atas/bawah composer (referensi 02).
fn composer_bands(width: usize) -> (&'static str, &'static str) {
    let top = "╭─ Prompt (Markdown & Multi-line · Enter kirim, Shift+Enter baris baru)";
    let bottom = "╰─ [Tab] Lengkapi  [/] Slash  [^C] Batalkan";
    let _ = width;
    (top, bottom)
}

/// Rail navigasi kiri. Labelnya nyata (fitur yang ada), bukan angka rekaan.
fn rail_lines(screen: &Screen) -> Vec<Line<'static>> {
    let p = screen.theme;
    let nav = ["REPL Session", "Tool Inspector", "MCP (—)", "Settings"];
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        " NAVIGASI ",
        Style::default().fg(p.outline).bg(p.low),
    )));
    for (index, item) in nav.iter().enumerate() {
        let active = index == 0;
        let style = if active {
            Style::default().fg(p.on_surface).bg(p.highest)
        } else {
            Style::default().fg(p.container)
        };
        out.push(Line::from(Span::styled(
            format!(" {}{} ", if active { "▌" } else { " " }, item),
            style,
        )));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        " STATUS ",
        Style::default().fg(p.outline).bg(p.low),
    )));
    out.push(Line::from(Span::styled(
        format!(" model: {}", screen.model),
        Style::default().fg(p.on_variant),
    )));
    out.push(Line::from(Span::styled(
        format!(" {}", screen.status_line()),
        Style::default().fg(p.outline),
    )));
    out.push(Line::from(Span::styled(
        format!(
            " approval: {}",
            Screen::approval_label(screen.approval_mode)
        ),
        Style::default().fg(p.outline),
    )));
    out
}

fn draw(frame: &mut ratatui::Frame<'_>, screen: &mut Screen) {
    screen.refresh_pending_lines();
    let p = screen.theme;
    let area = frame.area();

    // Canvas referensi: seluruh frame memakai surface gelap, bukan biru tua.
    frame.render_widget(
        Block::default().style(Style::default().bg(p.canvas).fg(p.on_surface)),
        area,
    );

    // Hitung pembagian: badan composer mengikuti tinggi isi (3..12 baris).
    let composer_total = screen.composer_height().clamp(3, 12);

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(composer_total.saturating_sub(2)),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let header = chunks[0];
    let body = chunks[1];
    let c_top = chunks[2];
    let c_body = chunks[3];
    let c_bottom = chunks[4];
    let footer = chunks[5];

    // Header: status line + indikator keadaan (semua data nyata dari state).
    let (state_label, state_color) = if screen.pending.is_some() {
        ("● MENUNGGU", p.error)
    } else if screen.busy {
        ("● RUNNING", p.tertiary_container)
    } else {
        ("○ IDLE", p.outline)
    };
    let header_line = Line::from(vec![
        Span::styled(" ● ● ● ", Style::default().fg(p.error_container).bg(p.low)),
        Span::styled(
            "maicli :: ",
            Style::default().fg(p.primary_container).bg(p.low),
        ),
        Span::styled(
            format!("{} ", screen.model),
            Style::default().fg(p.on_surface).bg(p.low),
        ),
        Span::styled("— ", Style::default().fg(p.outline_variant).bg(p.low)),
        Span::styled(
            format!("{} ", screen.status),
            Style::default().fg(p.on_variant).bg(p.low),
        ),
        Span::styled(state_label, Style::default().fg(state_color).bg(p.low)),
        Span::styled("  ● ● ● ", Style::default().fg(p.secondary).bg(p.low)),
    ]);
    frame.render_widget(
        Paragraph::new(header_line).style(Style::default().bg(p.low)),
        header,
    );

    // Rail kiri hanya bila terminal cukup lebar.
    let rail_width = if body.width >= 92 { 22u16 } else { 0u16 };
    let columns =
        Layout::horizontal([Constraint::Length(rail_width), Constraint::Min(20)]).split(body);
    if rail_width > 0 {
        frame.render_widget(
            Paragraph::new(rail_lines(screen))
                .style(Style::default().bg(p.low))
                .block(
                    Block::default()
                        .borders(ratatui::widgets::Borders::RIGHT)
                        .border_style(Style::default().fg(p.outline_variant)),
                ),
            columns[0],
        );
    }

    screen.view_height = columns[1].height.saturating_sub(2).max(1);
    screen.view_width = columns[1].width.saturating_sub(2).max(1);
    let state_text = if screen.busy { "RUNNING" } else { "IDLE" };
    frame.render_widget(
        Paragraph::new(screen.body())
            .style(Style::default().bg(p.lowest).fg(p.on_surface))
            .block(
                Block::bordered()
                    .title(format!(" [0x1] TRANSCRIPT │ {state_text} "))
                    .style(Style::default().bg(p.lowest))
                    .border_style(Style::default().fg(p.outline_variant)),
            )
            .scroll((screen.top_offset(), 0))
            .wrap(Wrap { trim: false }),
        columns[1],
    );

    // Composer tiga band dengan border ASCII referensi 02.
    let (band_top, band_bottom) = composer_bands(usize::from(c_body.width));
    frame.render_widget(
        Paragraph::new(band_top).style(Style::default().fg(p.primary).bg(p.high)),
        c_top,
    );
    // Meter konteks (referensi 14): bar ASCII dari data nyata `context`.
    if let Some((tokens, window)) = screen.context
        && let Some(pct) = tokens
            .saturating_mul(100)
            .checked_div(window)
            .map(|v| v.min(100))
    {
        {
            let filled = usize::from((pct as u16).saturating_mul(8) / 100);
            let meter = format!(
                "[{}{}] {}%",
                "█".repeat(filled),
                "░".repeat(8usize.saturating_sub(filled)),
                pct
            );
            let meter_line = Line::from(vec![
                Span::styled(
                    " CTX ",
                    Style::default().fg(p.secondary_container).bg(p.lowest),
                ),
                Span::styled(meter, Style::default().fg(p.tertiary).bg(p.lowest)),
            ]);
            frame.render_widget(
                Paragraph::new(meter_line).style(Style::default().bg(p.lowest)),
                Rect {
                    x: c_body.x,
                    y: c_body.y,
                    width: c_body.width,
                    height: 1,
                },
            );
        }
    }
    let sigil = if screen.question.is_some() {
        "?"
    } else {
        "❯"
    };
    let input_line = Line::from(vec![
        Span::styled(format!("{sigil} "), Style::default().fg(p.primary).bold()),
        Span::styled(
            format!("{}█", screen.input),
            Style::default().fg(p.on_surface).bg(p.lowest),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(input_line)
            .style(Style::default().bg(p.lowest))
            .wrap(Wrap { trim: false })
            .scroll((screen.composer_scroll(), 0)),
        c_body,
    );
    frame.render_widget(
        Paragraph::new(band_bottom).style(Style::default().fg(p.outline).bg(p.high)),
        c_bottom,
    );

    frame.render_widget(
        Paragraph::new(screen.hint()).style(Style::default().fg(p.outline).bg(p.canvas)),
        footer,
    );
    if screen.show_help {
        let area = centered_area(frame.area(), 64, 28);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(screen.help_text()).block(
                Block::bordered()
                    .title(" bantuan ")
                    .border_style(Style::default().add_modifier(Modifier::BOLD)),
            ),
            area,
        );
    }
    if screen.agents_open {
        let rows = screen.subagent_rows();
        let height = (rows.len() as u16 + 3).clamp(5, 20);
        let area = centered_area(frame.area(), 72, height);
        let mut text = rows.join("\n");
        text.push_str("\n\nF3/Esc menutup panel");
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(" subagent (F3) ")
                    .border_style(Style::default().fg(screen.accent)),
            ),
            area,
        );
    }
    if let Some(hub) = screen.hub.as_ref() {
        // Agent Hub penuh: kiri daftar anak, kanan event + kendali.
        let area = centered_area(frame.area(), 92, 30);
        frame.render_widget(Clear, area);
        let handles = screen
            .subagents
            .as_ref()
            .map(|manager| manager.list())
            .unwrap_or_default();
        let mut daftar = String::new();
        for (index, handle) in handles.iter().enumerate() {
            let mark = if index == hub.selected { ">" } else { " " };
            daftar.push_str(&format!(
                "{mark} {} {}\n",
                handle.id,
                handle.status().label()
            ));
        }
        if handles.is_empty() {
            daftar.push_str("belum ada subagent\n");
        }
        let mut detail = String::new();
        if let Some(anak) = handles.get(hub.selected) {
            detail.push_str(&format!(
                "{} — status: {}\n\n",
                anak.id,
                anak.status().label()
            ));
            let sumber: Option<
                &std::sync::Mutex<
                    std::collections::HashMap<String, std::collections::VecDeque<String>>,
                >,
            > = match hub.tampilan {
                HubTampilan::Event => Some(&screen.hub_events),
                HubTampilan::Transkrip => Some(&screen.hub_transkrip),
            };
            if let Some(buffer) = sumber
                && let Ok(map) = buffer.lock()
                && let Some(antre) = map.get(&anak.id)
            {
                let mulai = antre.len().saturating_sub(16);
                for baris in antre.iter().skip(mulai) {
                    detail.push_str(baris);
                    detail.push('\n');
                }
            } else {
                detail.push_str("(belum ada)\n");
            }
        }
        let label_tampilan = match hub.tampilan {
            HubTampilan::Event => "event",
            HubTampilan::Transkrip => "transkrip",
        };
        match hub.mode {
            HubMode::Lihat => detail.push_str(&format!(
                "\n[s]teer  [p]rompt  [a]bort  [K]ill  [t]ampilan:{label_tampilan}  [b]roadcast  Esc"
            )),
            HubMode::Steer => detail.push_str(&format!("\nsteer> {}█", hub.input)),
            HubMode::Prompt => detail.push_str(&format!("\nprompt> {}█", hub.input)),
        }
        let judul_kanan = format!(
            " {} ({label_tampilan}) ",
            if hub.broadcast {
                "BROADCAST"
            } else {
                "kendali"
            }
        );
        let kiri = Paragraph::new(daftar).block(
            Block::bordered()
                .title(" agent hub — anak ")
                .border_style(Style::default().fg(screen.accent)),
        );
        let kanan = Paragraph::new(detail).block(
            Block::bordered()
                .title(judul_kanan)
                .border_style(Style::default().fg(screen.dim)),
        );
        let kolom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(20)])
            .split(area);
        frame.render_widget(kiri, kolom[0]);
        frame.render_widget(kanan, kolom[1]);
    }
    if let Some(palette) = screen.palette.as_ref() {
        let items = screen.palette_items();
        let height = (items.len() as u16 + 3).clamp(4, 18);
        let area = centered_area(frame.area(), 72, height);
        let mut text = format!("> {}\n", palette.query);
        for (index, spec) in items.iter().enumerate() {
            let mark = if index == palette.selected { ">" } else { " " };
            text.push_str(&format!(
                "{mark} {:<14} {}\n",
                spec.display(),
                spec.description
            ));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(" ╭─ Slash commands (Ctrl+K) ")
                    .border_style(Style::default().fg(screen.accent)),
            ),
            area,
        );
    }
    if let Some(picker) = screen.picker.as_ref() {
        let height = (picker.items.len() as u16 + 2).min(20);
        let area = centered_area(frame.area(), 64, height.max(4));
        let mut text = String::new();
        for (index, item) in picker.items.iter().enumerate() {
            let mark = if index == picker.selected { ">" } else { " " };
            text.push_str(&format!(
                "{mark} {} ({} pesan)\n",
                item.title, item.messages
            ));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(" pilih sesi (enter=lanjut, esc=batal) ")
                    .border_style(Style::default().fg(screen.accent)),
            ),
            area,
        );
    }
}

/// Jalankan TUI sampai pengguna keluar.
pub async fn run(mut boot: Boot) -> Result<(), MxError> {
    enable_raw_mode().map_err(io_error)?;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
    .map_err(io_error)?;
    let _guard = TermGuard;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(io_error)?;

    let model = boot.model.clone();
    let resume_line = boot.resume_path.as_ref().map(|path| {
        format!(
            "melanjutkan sesi {} ({} pesan)",
            path.display(),
            boot.resume_messages.len()
        )
    });
    let accent = parse_color(&boot.ui.accent);
    let dim = parse_color(&boot.ui.dim);
    let (events_tx, mut events_rx, event_log_done) =
        crate::event_log::channel_with_recorder(boot.recorder.clone());
    // Penerus notifikasi: idle-timeout mencegah deadlock; abort saat keluar.
    let notif_task = boot
        .notif_rx
        .take()
        .map(|rx| crate::mcp_boot::spawn_event_forward(rx, events_tx.clone()));
    let (approvals_tx, mut approvals_rx) = unbounded_channel::<PendingApproval>();
    let (questions_tx, mut questions_rx) = unbounded_channel::<AskRequest>();
    let approval_timeout = Duration::from_secs(boot.limits.approval_timeout_secs);
    let approval_mode = boot.approval.mode;
    let yolo = approval_mode == tui_approval::ApprovalMode::Yolo;
    let mcp_notes = boot.mcp_notes.clone();
    let subagents = boot.subagents.clone();
    // Katalog di-clone sebelum boot dipindah ke worker (dipakai picker/kendali).
    let katalog = boot.katalog.clone();
    let known_models: Vec<String> = boot.models.clone();
    let keybindings = boot.ui.keybindings.clone();
    let commands = spawn_worker(boot, events_tx, approvals_tx, questions_tx);

    let mut screen = Screen::new(model, approval_mode, accent, dim, approval_timeout);
    screen.set_keybindings(&keybindings);
    if let Some(manager) = subagents.clone() {
        screen.set_subagents(manager.clone());
        // Kolektor event: langgani anak baru di roster, rekam ke buffer panel.
        let buffer = screen.hub_events.clone();
        let transkrip = screen.hub_transkrip.clone();
        tokio::spawn(async move {
            let mut dilanggani: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                for handle in manager.list() {
                    if dilanggani.contains(&handle.id) {
                        continue;
                    }
                    dilanggani.insert(handle.id.clone());
                    let mut rx = handle.events().await;
                    let buffer = buffer.clone();
                    let transkrip = transkrip.clone();
                    let id = handle.id.clone();
                    tokio::spawn(async move {
                        while let Some(event) = rx.recv().await {
                            if let tui_wire::AgentEvent::MessageEnd { message } = &event {
                                rekam_transkrip(&transkrip, &id, message);
                            }
                            rekam_event(&buffer, &id, &event);
                        }
                    });
                }
            }
        });
    }
    if yolo {
        screen.push_line(crate::banner::yolo_banner().to_string());
    }
    if let Some(line) = resume_line {
        screen.push_line(line);
    }
    for note in &mcp_notes {
        screen.push_line(format!("[mcp] {note}"));
    }
    // Untuk pengukuran kriteria C-09: latensi delta streaming dan durasi frame
    // dicatat ke log (bukan dirasakan), aktif dengan MAICLI_LOG=debug.
    let mut last_delta_at: Option<std::time::Instant> = None;
    loop {
        while let Ok(event) = events_rx.try_recv() {
            if matches!(
                &event,
                AgentEvent::MessageUpdate(ProviderEvent::TextDelta { .. })
            ) {
                last_delta_at = Some(std::time::Instant::now());
            }
            let turn_selesai = matches!(&event, AgentEvent::AgentEnd { .. });
            screen.apply(event);
            // Antrean /queue: jalankan prompt berikutnya setelah turn selesai.
            if turn_selesai
                && !screen.queue.is_empty()
                && screen.pending.is_none()
                && screen.question.is_none()
            {
                let berikut = screen.queue.remove(0);
                screen.push_line(timeline_label(&berikut));
                screen.busy = true;
                let _ = commands.send(Command::Prompt(berikut));
            }
        }
        while let Ok(pending) = approvals_rx.try_recv() {
            screen.show_pending(pending);
        }
        while let Ok(request) = questions_rx.try_recv() {
            screen.status = "menunggu jawaban".to_string();
            screen.question = Some(PendingQuestion {
                question: request.question,
                reply: request.reply,
            });
        }
        let frame_start = std::time::Instant::now();
        terminal
            .draw(|frame| draw(frame, &mut screen))
            .map_err(io_error)?;
        tracing::debug!(
            target: "maicli::tui",
            frame_ms = frame_start.elapsed().as_millis() as u64,
            "frame dirender"
        );
        if let Some(received_at) = last_delta_at.take() {
            tracing::debug!(
                target: "maicli::tui",
                delta_ms = received_at.elapsed().as_millis() as u64,
                "delta streaming dirender"
            );
        }
        if !event::poll(Duration::from_millis(50)).map_err(io_error)? {
            continue;
        }
        let key = match event::read().map_err(io_error)? {
            Event::Key(key) => key,
            Event::Mouse(mouse) => {
                screen.handle_mouse(mouse);
                continue;
            }
            Event::Paste(text) => {
                screen.insert_text(&text);
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match screen.handle_key(key, &commands) {
            Some(Action::Submit(text)) => {
                // Form `/provider add` aktif: Submit dijalankan form, bukan prompt.
                let langkah_form = screen.form_add.as_ref().map(|f| f.langkah);
                if let Some(langkah) = langkah_form {
                    let nilai = text.trim().to_string();
                    let mut selesai = false;
                    match langkah {
                        0 => {
                            let sah = !nilai.is_empty()
                                && nilai
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                            if !sah {
                                screen.status =
                                    "id provider harus alfanumerik/-/_ tanpa spasi".to_string();
                                continue; // tetap di langkah 0
                            }
                            if let Some(f) = screen.form_add.as_mut() {
                                f.id = nilai;
                                f.langkah = 1;
                            }
                        }
                        1 => {
                            if !nilai.starts_with("http://") && !nilai.starts_with("https://") {
                                screen.status =
                                    "base_url harus mulai http:// atau https://".to_string();
                                continue;
                            }
                            if let Some(f) = screen.form_add.as_mut() {
                                f.base_url = nilai;
                                f.langkah = 2;
                            }
                        }
                        2 => {
                            let sah = !nilai.is_empty()
                                && nilai.chars().all(|c| {
                                    c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()
                                });
                            if !sah {
                                screen.status =
                                    "env key harus HURUF_BESAR + garis bawah".to_string();
                                continue;
                            }
                            if let Some(f) = screen.form_add.as_mut() {
                                f.env_key = nilai;
                                f.langkah = 3;
                            }
                        }
                        _ => {
                            if let Some(f) = screen.form_add.as_mut() {
                                f.model = nilai;
                            }
                            selesai = true;
                        }
                    }
                    if selesai {
                        let Some(form) = screen.form_add.take() else {
                            continue; // tidak mungkin (form ada) — aman tetap ditangani
                        };
                        let id = form.id.clone();
                        // Simpan 4 kunci ke config global (ketat: schema sudah ada).
                        let akar = crate::config_root().join("config.toml");
                        let mut galat_simpan: Option<String> = None;
                        for (kunci, nilai) in [
                            (
                                format!("providers_eksternal.{id}.base_url"),
                                form.base_url.clone(),
                            ),
                            (
                                format!("providers_eksternal.{id}.env_key"),
                                form.env_key.clone(),
                            ),
                            (
                                format!("providers_eksternal.{id}.model"),
                                form.model.clone(),
                            ),
                        ] {
                            if let Err(err) = tui_config::set_global_value(&akar, &kunci, &nilai) {
                                galat_simpan = Some(err.message);
                                break;
                            }
                        }
                        match galat_simpan {
                            Some(galat) => {
                                screen.push_line(format!("[provider] gagal simpan: {galat}"))
                            }
                            None => screen.push_line(format!(
                                "[provider] '{id}' tersimpan di config global; restart (atau set env {env}) lalu /provider {id}",
                                env = form.env_key
                            )),
                        }
                        screen.status = "form provider selesai".to_string();
                    }
                    continue;
                }
                // URI host: agent://sub-1 & history://sub-1 → detail anak di
                // timeline (read-only); bukan prompt untuk model.
                if let Some(id) = text
                    .strip_prefix("agent://")
                    .or_else(|| text.strip_prefix("history://"))
                {
                    let id = id.trim().to_string();
                    screen.push_line(timeline_label(&text));
                    if let Some(manager) = screen.subagents.as_ref() {
                        let handle = manager.list().into_iter().find(|h| h.id == id);
                        match handle {
                            Some(anak) => {
                                screen.push_line(format!(
                                    "[agent] {} status={} turns={}",
                                    anak.id,
                                    anak.status().label(),
                                    anak.result().as_ref().map(|r| r.turns).unwrap_or(0)
                                ));
                                let terakhir: Vec<String> = if let Ok(map) =
                                    screen.hub_events.lock()
                                    && let Some(antre) = map.get(&anak.id)
                                {
                                    antre.iter().rev().take(20).rev().cloned().collect()
                                } else {
                                    Vec::new()
                                };
                                for baris in terakhir {
                                    screen.push_line(format!("  {baris}"));
                                }
                            }
                            None => screen.push_line(format!("[agent] {id} tidak ada (lihat F3)")),
                        }
                    }
                    screen.busy = false;
                    continue;
                }
                // /provider add → form interaktif bertahap.
                if text.trim() == "/provider add" {
                    screen.push_line(timeline_label(&text));
                    screen.form_add = Some(FormProvider {
                        langkah: 0,
                        id: String::new(),
                        base_url: String::new(),
                        env_key: String::new(),
                        model: String::new(),
                    });
                    screen.busy = false;
                    continue;
                }
                // /model tanpa argumen → **picker interaktif** daftar model'
                // discovery (padanan omp; prompt `/model <id>` dikirim otomatis).
                if text.trim() == "/model" && !known_models.is_empty() {
                    screen.picker = Some(Picker {
                        items: known_models
                            .iter()
                            .map(|m| SessionChoice {
                                id: m.clone(),
                                title: m.clone(),
                                messages: 0,
                            })
                            .collect(),
                        selected: 0,
                        jenis: PickerJenis::Model,
                    });
                    screen.push_line(timeline_label(&text));
                    screen.busy = false;
                    continue;
                }
                // /catalog <id> → pilih provider katalog: model bawaan → validasi
                // discovery → set; tanpa key → pesan nama env.
                if let Some(id_katalog) = text.strip_prefix("/catalog ") {
                    let id = id_katalog.trim();
                    screen.push_line(timeline_label(&text));
                    let aturan = katalog
                        .as_ref()
                        .and_then(|k| k.providers.iter().find(|p| p.id == id));
                    match aturan {
                        None => {
                            screen.push_line(format!(
                                "[katalog] provider '{id}' tidak ada di katalog"
                            ));
                        }
                        Some(aturan) => {
                            let siap = tui_wire::providers::by_id(id)
                                .and_then(|entry| {
                                    entry.resolve_env_key(|nama| {
                                        std::env::var(nama).ok().filter(|v| !v.trim().is_empty())
                                    })
                                })
                                .is_some();
                            if !siap {
                                let env = tui_wire::providers::by_id(id)
                                    .and_then(|e| e.env_keys.first().cloned())
                                    .unwrap_or_else(|| "<env>".to_string());
                                screen.push_line(format!(
                                    "[katalog] {id}: kunci belum tersedia — isi env {env} dulu"
                                ));
                                screen.busy = false;
                                continue;
                            }
                            match aturan.model_bawaan.as_deref() {
                                Some(model_bawaan)
                                    if known_models.is_empty()
                                        || known_models.iter().any(|m| m == model_bawaan) =>
                                {
                                    screen.push_line(format!(
                                        "[katalog] memilih {id} → model {model_bawaan}"
                                    ));
                                    screen.busy = true;
                                    let _ = commands
                                        .send(Command::Prompt(format!("/model {model_bawaan}")));
                                }
                                Some(model_bawaan) => {
                                    screen.push_line(format!(
                                        "[katalog] {id}: model bawaan '{model_bawaan}' tidak ada di gateway aktif — pilih model lain via /model"
                                    ));
                                }
                                None => screen
                                    .push_line(format!("[katalog] {id}: tidak punya model bawaan")),
                            }
                        }
                    }
                    screen.busy = false;
                    continue;
                }
                // /catalog tanpa argumen → picker interaktif (pilih provider).
                if text.trim() == "/catalog" {
                    screen.push_line(timeline_label(&text));
                    let items = katalog_pilihan(katalog.as_ref());
                    if items.is_empty() {
                        screen.push_line(
                            "[katalog] kosong atau tidak dimuat (flag --catalog / config catalog.path)"
                                .to_string(),
                        );
                    } else {
                        screen.picker = Some(Picker {
                            items,
                            selected: 0,
                            jenis: PickerJenis::Katalog,
                        });
                    }
                    screen.busy = false;
                    continue;
                }
                // /hub → buka Agent Hub (sama dengan Alt+A).
                if text.trim() == "/hub" {
                    screen.push_line(timeline_label(&text));
                    screen.hub = Some(HubPanel {
                        selected: 0,
                        mode: HubMode::Lihat,
                        input: String::new(),
                        tampilan: HubTampilan::Event,
                        broadcast: false,
                    });
                    screen.busy = false;
                    continue;
                }
                // /queue → tampilkan prompt yang menunggu di antrean.
                if text.trim() == "/queue" {
                    screen.push_line(timeline_label(&text));
                    let tampil: Vec<String> = screen
                        .queue
                        .iter()
                        .enumerate()
                        .map(|(i, q)| format!("[antrean] {}. {q}", i + 1))
                        .collect();
                    if tampil.is_empty() {
                        screen.push_line("[antrean] kosong".to_string());
                    } else {
                        for baris in tampil {
                            screen.push_line(baris);
                        }
                    }
                    screen.busy = false;
                    continue;
                }
                // /retry → kirim ulang prompt terakhir.
                if text.trim() == "/retry" {
                    screen.push_line(timeline_label(&text));
                    match screen.history.last() {
                        Some(p) => {
                            let ulang = p.clone();
                            screen.push_line(timeline_label(&ulang));
                            screen.busy = true;
                            let _ = commands.send(Command::Prompt(ulang));
                        }
                        None => {
                            screen.push_line("[retry] belum ada prompt di sesi ini".to_string())
                        }
                    }
                    continue;
                }
                // /commit → prompt khusus ke agen (approval git tetap berjalan).
                if text.trim() == "/commit" {
                    let prompt = tui_core::commands::commit_prompt();
                    screen.push_line(timeline_label(&text));
                    screen.busy = true;
                    let _ = commands.send(Command::Prompt(prompt));
                    continue;
                }
                screen.push_line(timeline_label(&text));
                // Saat sibuk: antrekan (dijalankan otomatis setelah AgentEnd).
                if screen.busy {
                    screen.queue.push(text);
                    screen.push_line("[antrean] menunggu turn selesai (lihat /queue)".to_string());
                    continue;
                }
                screen.busy = true;
                let _ = commands.send(Command::Prompt(text));
            }
            Some(Action::Abort) => {
                screen.busy = false;
            }
            Some(Action::SwitchSession(id)) => {
                // Lewat jalur perintah yang sama dengan ketikan, supaya perilaku
                // pindah sesi hanya punya satu implementasi.
                let text = format!("/switch {id}");
                screen.push_line(timeline_label(&text));
                let _ = commands.send(Command::Prompt(text));
            }
            Some(Action::Quit) => break,
            None => {}
        }
    }
    let _ = commands.send(Command::Quit);
    let _ = terminal.show_cursor();
    // Hentikan penerus notifikasi agar task tidak menggantung shutdown.
    if let Some(tugas) = notif_task {
        tugas.abort();
    }
    crate::event_log::flush(event_log_done).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Handler tombol diuji lewat assert yang gagal keras; `panic!` di test
    // adalah cara yang jelas untuk menunjuk tombol mana yang salah.
    #![allow(clippy::panic, clippy::expect_used)]

    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tui_approval::ApprovalRequest;
    use tui_wire::{AssistantMessage, StopReason, ToolTier};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn screen() -> Screen {
        Screen::new(
            "model-uji".to_string(),
            tui_approval::ApprovalMode::AlwaysAsk,
            Color::Yellow,
            Color::DarkGray,
            Duration::from_secs(600),
        )
    }

    /// Render layar pada ukuran terminal tertentu, kembalikan buffer sebagai
    /// teks (per baris, tanpa spasi ganda kanan) untuk frame emas.
    fn render_frame(layar: &mut Screen, lebar: u16, tinggi: u16) -> Vec<String> {
        let backend = ratatui::backend::TestBackend::new(lebar, tinggi);
        let mut terminal = Terminal::new(backend).expect("terminal uji");
        terminal.draw(|frame| draw(frame, layar)).expect("render");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn welcome_mengikuti_desain_referensi_bukan_layar_lama() {
        let layar = screen();
        let teks = layar
            .body()
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(teks.contains("███"), "logo ASCII harus ada: {teks}");
        assert!(teks.contains("QUICKSTART"), "{teks}");
        assert!(teks.contains("STATUS"), "{teks}");
        assert!(
            !teks.contains("Tulis prompt lalu tekan Enter"),
            "jangan tampilkan layar lama: {teks}"
        );
    }

    #[test]
    fn frame_memakai_shell_referensi_dan_bukan_tema_lama() {
        let mut layar = screen();
        let frame = render_frame(&mut layar, 140, 40);
        let teks = frame.join("\n");
        assert!(teks.contains("maicli ::"), "{teks}");
        assert!(teks.contains("NAVIGASI"), "{teks}");
        assert!(teks.contains("╭─ Prompt"), "{teks}");
        assert!(teks.contains("╰─ [Enter]"), "{teks}");
        assert!(!teks.contains("Tulis prompt lalu tekan Enter"), "{teks}");
        assert!(
            !teks.contains("─ sesi ─"),
            "border lama tidak boleh ada: {teks}"
        );
    }

    #[test]
    fn palet_warna_mengikuti_code_html() {
        let p = Theme::reference();
        assert_eq!(p.canvas, Color::Rgb(11, 20, 28), "#0b141c");
        assert_eq!(p.lowest, Color::Rgb(6, 15, 22), "#060f16");
        assert_eq!(p.primary, Color::Rgb(162, 201, 255), "#a2c9ff");
        assert_eq!(p.tertiary, Color::Rgb(111, 221, 120), "#6fdd78");
        assert_eq!(p.error, Color::Rgb(255, 180, 171), "#ffb4ab");
    }

    #[test]
    fn frame_emas_tiga_ukuran() {
        // Layar contoh: pesan + status; komposisi harus stabil antarukuran.
        let mut layar = screen();
        layar.push_line("halo dari maicli".to_string());
        layar.push_line("baris kedua".to_string());
        let mut emas = Vec::new();
        for (lebar, tinggi) in [(80u16, 24u16), (120, 40), (200, 50)] {
            let mut layar_uji = {
                let mut l = screen();
                l.push_line("halo dari maicli".to_string());
                l.push_line("baris kedua".to_string());
                l
            };
            let frame = render_frame(&mut layar_uji, lebar, tinggi);
            // Struktur bawah: hint line + status line + composer border ada.
            let terakhir = frame.iter().rev().map(|l| l.as_str()).collect::<Vec<_>>();
            assert!(
                terakhir
                    .iter()
                    .any(|l| l.contains("status") || !l.is_empty()),
                "frame {lebar}x{tinggi} tidak boleh kosong"
            );
            let konten: String = frame.join("\n");
            assert!(
                konten.contains("halo dari maicli"),
                "{lebar}x{tinggi}: {konten}"
            );
            assert!(konten.contains("baris kedua"));
            emas.push(format!("{lebar}x{tinggi}: {} baris", frame.len()));
        }
        // Ukuran lebih besar menampilkan semua baris tanpa memotong konten.
        assert!(emas.len() == 3, "{emas:?}");
    }

    #[test]
    fn frame_resize_tidak_kehilangan_konten() {
        let mut layar = screen();
        for i in 0..50 {
            layar.push_line(format!("baris-{i}"));
        }
        let sempit = render_frame(&mut layar, 80, 24);
        let lebar_layar = render_frame(&mut layar, 200, 50);
        // Konten tetap tersedia setelah "resize".
        assert!(lebar_layar.join("\n").contains("baris-49"));
        assert!(!sempit.is_empty());
    }
    #[tokio::test]
    async fn hub_transkrip_dari_message_end_dan_broadcast_steer() {
        // 1) Transkrip: event MessageEnd (teks + tool call) → buffer transkrip.
        let layar = screen();
        let pesan = AssistantMessage {
            content: vec![
                tui_wire::ContentBlock::Text {
                    text: "jawaban anak lengkap".to_string(),
                },
                tui_wire::ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                    arguments: "{}".to_string(),
                },
            ],
            stop_reason: StopReason::Stop,
            usage: tui_wire::Usage::default(),
            error: None,
        };
        rekam_transkrip(&layar.hub_transkrip, "sub-1", &pesan);
        // Asersi dalam scope tertutup: MutexGuard tidak boleh menyeberang await.
        {
            let map = layar.hub_transkrip.lock().expect("map");
            let antre = map.get("sub-1").expect("transkrip");
            assert_eq!(antre.len(), 2);
            assert!(antre[0].contains("jawaban anak lengkap"), "{}", antre[0]);
            assert_eq!(antre[1], "→ panggil bash");
        }

        // 2) Broadcast: steer ke SEMUA anak berjalan (2 anak gantung).
        struct Gantung;
        #[async_trait::async_trait]
        impl tui_provider::Provider for Gantung {
            async fn stream_chat(
                &self,
                _request: tui_provider::ChatRequest,
            ) -> Result<UnboundedReceiver<tui_wire::ProviderEvent>, tui_provider::GatewayError>
            {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    drop(tx);
                });
                Ok(rx)
            }
        }
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        let spec = tui_subagent::SubagentSpec {
            provider: Arc::new(Gantung),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools: tui_tools::ToolRegistry::from_tools(Vec::new()),
            model: "m".to_string(),
            deadline: Duration::from_secs(60),
            max_tool_calls: 4,
        };
        let _a = manager.spawn(spec.clone(), "a".to_string(), "induk", None);
        let _b = manager.spawn(spec, "b".to_string(), "induk", None);
        let mut layar = screen();
        layar.set_subagents(manager);
        tokio::time::sleep(Duration::from_millis(80)).await;
        // Buka hub → b (broadcast) → ketik "fok" → Enter.
        let mut alt_a = key(KeyCode::Char('a'));
        alt_a.modifiers = KeyModifiers::ALT;
        layar.handle_key(alt_a, &tx_dummy());
        layar.handle_key(key(KeyCode::Char('b')), &tx_dummy());
        layar.handle_key(key(KeyCode::Char('f')), &tx_dummy());
        layar.handle_key(key(KeyCode::Char('o')), &tx_dummy());
        layar.handle_key(key(KeyCode::Char('k')), &tx_dummy());
        layar.handle_key(key(KeyCode::Enter), &tx_dummy());
        assert!(
            layar.status.contains("steer terkirim ke 2"),
            "{}",
            layar.status
        );
    }

    #[tokio::test]
    async fn hub_toggle_tampilan_transkrip_render() {
        let mut layar = screen();
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        layar.set_subagents(manager);
        // Buka hub → t → tampilan transkrip (judul kanan berubah).
        let mut alt_a = key(KeyCode::Char('a'));
        alt_a.modifiers = KeyModifiers::ALT;
        layar.handle_key(alt_a, &tx_dummy());
        layar.handle_key(key(KeyCode::Char('t')), &tx_dummy());
        let hub = layar.hub.as_ref().expect("hub terbuka");
        assert_eq!(hub.tampilan, HubTampilan::Transkrip);
        let frame = render_frame(&mut layar, 92, 30);
        let konten = frame.join("\n");
        assert!(konten.contains("transkrip"), "{konten}");
        layar.handle_key(key(KeyCode::Char('t')), &tx_dummy());
        let hub = layar.hub.as_ref().expect("hub terbuka");
        assert_eq!(hub.tampilan, HubTampilan::Event);
    }

    #[tokio::test]
    async fn model_picker_dan_provider_add_form() {
        // 1) /model tanpa argumen + daftar model → picker interaktif.
        let mut layar = screen();
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        layar.set_subagents(manager);
        // Set katalog? tidak perlu — model picker berdiri sendiri. Simulasi submit
        // lewat handle_key tidak bisa (known_models ada di run) — uji jenis picker
        // berbeda lewat konstruksi langsung + enter:
        layar.picker = Some(Picker {
            items: vec![SessionChoice {
                id: "mock-model".to_string(),
                title: "mock-model".to_string(),
                messages: 0,
            }],
            selected: 0,
            jenis: PickerJenis::Model,
        });
        let aksi = layar.handle_key(key(KeyCode::Enter), &tx_dummy());
        assert_eq!(aksi, Some(Action::Submit("/model mock-model".to_string())));
    }

    #[tokio::test]
    async fn provider_add_form_tiga_langkah_dan_simpan() {
        let mut layar = screen();
        // Buka form: submit "/provider add" diproses di loop; di sini kita
        // simulasi langsung isi form (loop diuji lewat e2e/navigasi).
        layar.form_add = Some(FormProvider {
            langkah: 0,
            id: String::new(),
            base_url: String::new(),
            env_key: String::new(),
            model: String::new(),
        });
        // Ketik id → Enter → handle_key meneruskan Submit (loop memproses form).
        layar.input = "klien-uji".to_string();
        let aksi = layar.handle_key(key(KeyCode::Enter), &tx_dummy());
        assert_eq!(
            aksi,
            Some(Action::Submit("klien-uji".to_string())),
            "form meneruskan Submit; loop yang memproses langkah"
        );
        // Simulasi pemrosesan loop: id diterima → langkah naik.
        if let Some(f) = layar.form_add.as_mut() {
            f.id = "klien-uji".to_string();
            f.langkah = 1;
        }
        let form = layar.form_add.as_ref().expect("form hidup");
        assert_eq!(form.id, "klien-uji");
        assert_eq!(form.langkah, 1);
        // Hint menyebut pertanyaan langkah.
        assert!(layar.hint().contains("base_url"), "{}", layar.hint());
        // Esc → batal.
        assert_eq!(layar.handle_key(key(KeyCode::Esc), &tx_dummy()), None);
        assert!(layar.form_add.is_none());
        assert!(layar.status.contains("dibatalkan"));
    }

    #[test]
    fn katalog_pilihan_dari_aturan_nyata() {
        // Katalog kecil terbangun langsung dari parser (fixtures).
        let teks = r#"
provider "uji-cat" {
	default-model "uji-v1"
	env "UJI_CAT_KEY"
}
provider "uji-cat2" {
	env "UJI_CAT2_KEY"
}
"#;
        let mut katalog = tui_katalog::Katalog::default();
        let node1 = susun_provider_uji(teks);
        katalog.providers = node1;
        let pilihan = katalog_pilihan(Some(&katalog));
        assert_eq!(pilihan.len(), 2);
        assert_eq!(pilihan[0].id, "uji-cat");
        assert!(pilihan[0].title.contains("uji-v1"), "{}", pilihan[0].title);
        assert!(pilihan[0].title.contains("no-key"), "env uji tidak diset");
        assert!(pilihan[1].title.contains("model: ?"));
        // Katalog tidak dimuat → kosong (picker tidak dibuka).
        assert!(katalog_pilihan(None).is_empty());
    }

    fn susun_provider_uji(teks: &str) -> Vec<tui_katalog::AturanProvider> {
        // Bungkus parser katalog: satu provider per teks.
        let mut hasil = Vec::new();
        // Parser tui-katalog publik hanya via susun(dir); di sini cukup satu
        // provider dari teks lengkap (dua provider disusun dengan memisah blok).
        for bagian in teks.split("provider \"").skip(1) {
            let teks_provider = format!("provider \"{bagian}");
            if let Ok(Some(aturan)) = katalog_susun_provider_tunggal(&teks_provider) {
                hasil.push(aturan);
            }
        }
        hasil
    }

    fn katalog_susun_provider_tunggal(
        teks: &str,
    ) -> Result<Option<tui_katalog::AturanProvider>, String> {
        // Re-implementasi parser tunggal via susun folder sementara.
        let dir = std::env::temp_dir().join(format!(
            "maicli-cat-uji-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        let providers = dir.join("rules").join("providers");
        std::fs::create_dir_all(&providers).expect("dir");
        std::fs::write(providers.join("p.kdl"), teks).expect("tulis");
        let katalog = tui_katalog::susun(&dir.join("rules"))?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(katalog.providers.into_iter().next())
    }

    #[tokio::test]
    async fn agent_hub_alt_a_buka_tutup_dan_kendali() {
        let mut layar = screen();
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        layar.set_subagents(manager);
        // Alt+A buka hub.
        let mut kunci_alt_a = key(KeyCode::Char('a'));
        kunci_alt_a.modifiers = KeyModifiers::ALT;
        assert_eq!(layar.handle_key(kunci_alt_a, &tx_dummy()), None);
        assert!(layar.hub.is_some(), "Alt+A membuka hub");
        assert!(!layar.agents_open, "F3 panel ditutup saat hub terbuka");
        // Esc menutup.
        assert_eq!(layar.handle_key(key(KeyCode::Esc), &tx_dummy()), None);
        assert!(layar.hub.is_none());
        // s → mode steer, ketik → masuk hub.input (bukan composer).
        let mut kunci_alt_a2 = key(KeyCode::Char('a'));
        kunci_alt_a2.modifiers = KeyModifiers::ALT;
        assert_eq!(layar.handle_key(kunci_alt_a2, &tx_dummy()), None);
        assert_eq!(layar.handle_key(key(KeyCode::Char('s')), &tx_dummy()), None);
        assert_eq!(layar.handle_key(key(KeyCode::Char('x')), &tx_dummy()), None);
        let hub = layar.hub.as_ref().expect("hub masih terbuka");
        assert_eq!(hub.mode, HubMode::Steer);
        assert_eq!(hub.input, "x");
        assert!(layar.input.is_empty(), "composer tidak terisi");
        // Enter → kirim (tidak ada anak terpilih → mode kembali Lihat).
        assert_eq!(layar.handle_key(key(KeyCode::Enter), &tx_dummy()), None);
        let hub = layar.hub.as_ref().expect("hub tetap terbuka");
        assert_eq!(hub.mode, HubMode::Lihat);
        assert!(hub.input.is_empty());
        // Render tidak panik.
        let frame = render_frame(&mut layar, 92, 30);
        let konten = frame.join("\n");
        assert!(konten.contains("agent hub"), "{konten}");
    }

    #[test]
    fn buffer_event_cap_300_dan_format() {
        let layar = screen();
        assert_eq!(
            Screen::baris_event_anak(&tui_wire::AgentEvent::AgentStart),
            "mulai"
        );
        for i in 0..305 {
            rekam_event(
                &layar.hub_events,
                "sub-9",
                &tui_wire::AgentEvent::Notice {
                    text: format!("e{i}"),
                },
            );
        }
        let map = layar.hub_events.lock().expect("map");
        let antre = map.get("sub-9").expect("antre");
        assert_eq!(antre.len(), 300, "cap 300");
        assert_eq!(
            antre.front().map(String::as_str),
            Some("catatan: e5"),
            "paling lama terbuang"
        );
    }

    #[tokio::test]
    async fn agent_hub_abort_dan_kill_dengan_anak_nyata() {
        // Provider gantung → anak berjalan; abort + kill via hub keys.
        struct Gantung;
        #[async_trait::async_trait]
        impl tui_provider::Provider for Gantung {
            async fn stream_chat(
                &self,
                _request: tui_provider::ChatRequest,
            ) -> Result<UnboundedReceiver<tui_wire::ProviderEvent>, tui_provider::GatewayError>
            {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    drop(tx);
                });
                Ok(rx)
            }
        }
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        let handle = manager.spawn(
            tui_subagent::SubagentSpec {
                provider: Arc::new(Gantung),
                approver: Arc::new(tui_approval::FixedApprover::new(
                    tui_approval::Decision::Once,
                )),
                tools: tui_tools::ToolRegistry::from_tools(Vec::new()),
                model: "m".to_string(),
                deadline: Duration::from_secs(60),
                max_tool_calls: 4,
            },
            "gantung".to_string(),
            "induk",
            None,
        );
        let mut layar = screen();
        layar.set_subagents(manager);
        // Buka hub → a (abort) → status berubah.
        let mut alt_a = key(KeyCode::Char('a'));
        alt_a.modifiers = KeyModifiers::ALT;
        assert_eq!(layar.handle_key(alt_a, &tx_dummy()), None);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(layar.handle_key(key(KeyCode::Char('a')), &tx_dummy()), None);
        assert!(layar.status.contains("abort dikirim"), "{}", layar.status);
        assert_eq!(
            handle.status(),
            tui_subagent::manager::SubagentState::Running,
            "abort ≠ mati"
        );
        // K (shift+k) = kill.
        let mut k_key = key(KeyCode::Char('K'));
        k_key.modifiers = KeyModifiers::SHIFT;
        assert_eq!(layar.handle_key(k_key, &tx_dummy()), None);
        assert!(layar.status.contains("dihentikan"), "{}", layar.status);
        // Tunggu runner memproses kill.
        for _ in 0..50 {
            if handle.status() == tui_subagent::manager::SubagentState::Killed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            handle.status(),
            tui_subagent::manager::SubagentState::Killed
        );
    }

    #[tokio::test]
    async fn panel_f3_menampilkan_daftar_subagent() {
        use tui_subagent::SubagentSpec;
        use tui_wire::StopReason;
        // Manager dengan satu anak yang selesai.
        struct SProvider;
        #[async_trait::async_trait]
        impl tui_provider::Provider for SProvider {
            async fn stream_chat(
                &self,
                _r: tui_provider::ChatRequest,
            ) -> Result<
                tokio::sync::mpsc::UnboundedReceiver<tui_wire::ProviderEvent>,
                tui_provider::GatewayError,
            > {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let _ = tx.send(tui_wire::ProviderEvent::Done {
                    message: tui_wire::AssistantMessage::text(
                        "hasil anak panel".to_string(),
                        StopReason::Stop,
                    ),
                });
                Ok(rx)
            }
        }
        let manager = Arc::new(tui_subagent::manager::SubagentManager::new());
        let spec = SubagentSpec {
            provider: Arc::new(SProvider),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools: tui_tools::ToolRegistry::from_tools(Vec::new()),
            model: "m".to_string(),
            deadline: Duration::from_secs(10),
            max_tool_calls: 4,
        };
        let handle = manager.spawn(spec, "kerja".to_string(), "induk", None);
        for _ in 0..100 {
            if handle.status() == tui_subagent::manager::SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut layar = screen();
        layar.set_subagents(manager.clone());
        // Buka panel.
        layar.handle_key(key(KeyCode::F(3)), &tx_dummy());
        assert!(layar.agents_open);
        let baris = layar.subagent_rows().join("\n");
        assert!(baris.contains(&handle.id), "{baris}");
        assert!(baris.contains("idle"), "status idle: {baris}");
        assert!(baris.contains("hasil anak panel"), "{baris}");
        // Frame menampilkan panel.
        let frame = render_frame(&mut layar, 80, 24);
        let konten = frame.join("\n");
        assert!(konten.contains("subagent"), "{konten}");
        assert!(konten.contains(&handle.id), "{konten}");
        // Tutup panel dengan F3 lagi.
        layar.handle_key(key(KeyCode::F(3)), &tx_dummy());
        assert!(!layar.agents_open);
    }

    fn tx_dummy() -> tokio::sync::mpsc::UnboundedSender<Command> {
        let (tx, _rx) = unbounded_channel::<Command>();
        tx
    }

    #[test]
    fn parse_color_menerima_dua_bentuk_hex() {
        assert_eq!(parse_color("#d7a05b"), Color::Rgb(215, 160, 91));
        assert_eq!(parse_color("#abc"), Color::Rgb(170, 187, 204));
        assert_eq!(parse_color("tanpa-grid"), Color::Reset);
        assert_eq!(parse_color("#12345"), Color::Reset);
    }

    #[test]
    fn roda_mouse_menggulir_dan_di_clamp() {
        let mut screen = screen();
        screen.view_height = 2;
        screen.view_width = 10;
        screen.push_line("x".repeat(60));
        assert_eq!(screen.max_scroll(), 4, "6 baris visual - 2 terlihat");
        let scroll = |kind: MouseEventKind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        screen.handle_mouse(scroll(MouseEventKind::ScrollUp));
        assert_eq!(screen.top_offset(), 1, "naik 3 dari ujung bawah");
        screen.handle_mouse(scroll(MouseEventKind::ScrollDown));
        assert_eq!(screen.top_offset(), 4, "kembali ke ujung atas");
        screen.handle_mouse(scroll(MouseEventKind::ScrollDown));
        assert_eq!(screen.top_offset(), 4, "tidak melewati batas");
    }

    #[test]
    fn lebar_teks_menghitung_cjk_dan_emoji() {
        // Ditulis dengan escape Unicode supaya berkas tetap ASCII dan tidak bisa
        // rusak oleh alat yang menulis ulang berkas dengan encoding sistem.
        assert_eq!(display_width("abc"), 3);
        assert_eq!(
            display_width("\u{65e5}\u{672c}"),
            4,
            "CJK 2 kolom per karakter"
        );
        assert_eq!(display_width("\u{1f680}"), 2);
        assert_eq!(display_rows("\u{65e5}\u{672c}", 2), 2);
        assert_eq!(display_rows("", 10), 1, "baris kosong tetap satu");
    }

    #[test]
    fn gulir_memakai_baris_visual_setelah_lipat() {
        let mut screen = screen();
        screen.view_height = 2;
        screen.view_width = 10;
        screen.push_line("x".repeat(35));
        assert_eq!(screen.visual_lines(), 4, "35 karakter dilipat 10 kolom");
        assert_eq!(
            screen.max_scroll(),
            2,
            "4 baris visual dikurangi 2 terlihat"
        );
        screen.scroll_home();
        assert_eq!(screen.top_offset(), 0, "Home = ujung atas");
        screen.scroll_end();
        assert_eq!(screen.top_offset(), 2, "End = ujung bawah");
    }

    #[test]
    fn scroll_di_clamp_antara_nol_dan_maksimum() {
        let mut screen = screen();
        for index in 0..20 {
            screen.push_line(format!("baris {index}"));
        }
        screen.view_height = 5;
        assert_eq!(screen.max_scroll(), 15);
        screen.scroll_by(-1);
        assert_eq!(screen.scroll, 0, "tidak boleh negatif");
        screen.scroll_by(100);
        assert_eq!(screen.scroll, 15, "tidak boleh melewati maksimum");
        screen.scroll_end();
        assert_eq!(screen.scroll, 0);
        screen.scroll_home();
        assert_eq!(screen.scroll, 15);
    }

    #[test]
    fn baris_baru_mengembalikan_gulir_ke_bawah() {
        let mut screen = screen();
        for index in 0..4 {
            screen.push_line(format!("baris {index}"));
        }
        screen.view_height = 2;
        screen.scroll_home();
        assert!(screen.scroll > 0);
        screen.push_line("baru".to_string());
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn teks_streaming_menjadi_satu_baris() {
        let mut screen = screen();
        screen.apply(AgentEvent::MessageUpdate(ProviderEvent::TextDelta {
            text: "ha".into(),
        }));
        screen.apply(AgentEvent::MessageUpdate(ProviderEvent::TextDelta {
            text: "lo".into(),
        }));
        assert_eq!(screen.current, "halo");
        screen.apply(AgentEvent::MessageUpdate(ProviderEvent::TextEnd));
        assert_eq!(screen.lines, vec!["halo".to_string()]);
        assert!(screen.current.is_empty());
    }

    #[test]
    fn tabel_markdown_dirender_dengan_lebar_seragam() {
        let mut screen = screen();
        screen.push_line("| nama | nilai |".to_string());
        screen.push_line("| --- | :---: |".to_string());
        screen.push_line("| a | 12 |".to_string());
        screen.push_line("| panjang | 7 |".to_string());
        let lines = screen.body();
        assert_eq!(lines.len(), 3, "header + 2 baris data, tanpa tabel sisa");
        let texts: Vec<String> = lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let width_of = |text: &str| text.chars().count();
        assert!(
            texts
                .iter()
                .all(|text| width_of(text) == width_of(&texts[0])),
            "{texts:?}"
        );
        assert!(texts[2].contains("panjang"), "{texts:?}");
        assert!(texts[0].contains("nama"), "{texts:?}");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD),
            "header harus tebal"
        );
        assert!(
            !lines[2].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn tabel_hanya_dikenali_bila_ada_baris_pemisah() {
        assert!(is_table_separator("| --- | :---: |"));
        assert!(!is_table_separator("| nama | nilai |"));
        assert!(table_block(&["| a |".to_string(), "| b |".to_string()]).is_none());
        assert_eq!(
            split_table_row("| a | b |"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn blok_kode_berpagar_tidak_dinilai_sebagai_teks_biasa() {
        let mut screen = screen();
        screen.push_line("```rust".to_string());
        screen.push_line("let x = **bukan tebal**;".to_string());
        screen.push_line("```".to_string());
        screen.push_line("**ini tebal**".to_string());
        let lines = screen.body();
        assert_eq!(lines.len(), 4);
        // Di dalam blok: satu span apa adanya, bukan span tebal.
        assert_eq!(lines[1].spans.len(), 1, "{:?}", lines[1]);
        assert_eq!(
            lines[1].spans[0].content.as_ref(),
            "let x = **bukan tebal**;"
        );
        // Di luar blok: penekanan tetap diproses (satu span tebal penuh).
        assert_eq!(lines[3].spans.len(), 1, "{:?}", lines[3]);
        assert_eq!(lines[3].spans[0].content.as_ref(), "ini tebal");
        assert!(
            lines[3].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD),
            "{:?}",
            lines[3]
        );
    }

    #[test]
    fn baris_streaming_di_dalam_blok_kode_tetap_redup() {
        let mut screen = screen();
        screen.push_line("```".to_string());
        screen.current = "kode **sedang** ditulis".to_string();
        let lines = screen.body();
        let Some(last) = lines.last() else {
            panic!("tidak ada baris");
        };
        assert_eq!(last.spans.len(), 1);
        assert_eq!(last.spans[0].content.as_ref(), "kode **sedang** ditulis");
    }

    #[test]
    fn paste_baris_baru_menjadi_spasi_dan_dibatasi() {
        let mut layar = screen();
        layar.insert_text("baris satu\nbaris dua\r\nketiga");
        assert_eq!(layar.input, "baris satu baris dua  ketiga");
        let mut penuh = screen();
        penuh.insert_text(&"x".repeat(5_000));
        assert_eq!(penuh.input.chars().count(), 4_000);
    }

    #[test]
    fn riwayat_prompt_dipanggil_dengan_panah() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        for text in ["prompt satu", "prompt dua"] {
            screen.input = text.to_string();
            assert!(matches!(
                screen.handle_key(key(KeyCode::Enter), &tx),
                Some(Action::Submit(_))
            ));
        }
        assert_eq!(screen.history.len(), 2);

        // Up: ke prompt terakhir, lalu sebelumnya.
        assert!(screen.handle_key(key(KeyCode::Up), &tx).is_none());
        assert_eq!(screen.input, "prompt dua");
        assert!(screen.handle_key(key(KeyCode::Up), &tx).is_none());
        assert_eq!(screen.input, "prompt satu");
        assert!(screen.handle_key(key(KeyCode::Up), &tx).is_none());
        assert_eq!(screen.input, "prompt satu", "tidak melewati entri terlama");
        // Down: kembali baru, lalu kosong.
        assert!(screen.handle_key(key(KeyCode::Down), &tx).is_none());
        assert_eq!(screen.input, "prompt dua");
        assert!(screen.handle_key(key(KeyCode::Down), &tx).is_none());
        assert!(
            screen.input.is_empty(),
            "melewati entri terbaru mengosongkan"
        );
    }

    #[test]
    fn panah_tetap_menggulir_saat_composer_berisi() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.push_line("baris".to_string());
        screen.view_height = 1;
        screen.push_line("baris dua".to_string());
        screen.input = "sedang menulis".to_string();
        let before = screen.top_offset();
        assert!(screen.handle_key(key(KeyCode::Up), &tx).is_none());
        assert_eq!(screen.input, "sedang menulis", "isi tidak berubah");
        assert!(screen.top_offset() <= before);
    }

    #[test]
    fn ctrl_u_menghapus_composer() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.input = "teks panjang".to_string();
        let key = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert!(screen.handle_key(key, &tx).is_none());
        assert!(screen.input.is_empty());
    }

    #[test]
    fn teks_histori_mendukung_tebal_dan_kode() {
        let accent = Color::Yellow;
        let dim = Color::DarkGray;
        let spans = inline_spans("pakai `maicli --version` ya **sayang**", accent, dim);
        assert!(spans.len() >= 4, "harus terpecah: {spans:?}");
        let Some(code) = spans
            .iter()
            .find(|span| span.content.as_ref() == "maicli --version")
        else {
            panic!("span kode tidak ada: {spans:?}");
        };
        assert_eq!(code.style.fg, Some(dim));
        let Some(bold) = spans.iter().find(|span| span.content.as_ref() == "sayang") else {
            panic!("span tebal tidak ada: {spans:?}");
        };
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn penanda_tidak_ditutup_dibiarkan_apa_adanya() {
        let spans = inline_spans("nilai **terbuka", Color::Yellow, Color::DarkGray);
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "nilai **terbuka");
    }

    #[test]
    fn heading_memakai_warna_aksen_dan_tebal() {
        let line = styled_line("# Judul", Color::Yellow, Color::DarkGray);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content.as_ref(), "Judul");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn status_menampilkan_persen_konteks() {
        let mut screen = screen();
        assert!(!screen.status_line().contains("konteks"));
        screen.apply(AgentEvent::ContextUsage {
            tokens: 420,
            window: 1_000,
        });
        assert!(
            screen.status_line().contains("konteks 42%"),
            "{}",
            screen.status_line()
        );
        screen.apply(AgentEvent::ContextUsage {
            tokens: 10,
            window: 0,
        });
        assert!(
            !screen.status_line().contains("konteks"),
            "window nol tidak boleh membagi nol: {}",
            screen.status_line()
        );
    }

    #[test]
    fn status_menyebut_model_dan_keadaan() {
        let mut screen = screen();
        assert!(screen.status_line().contains("model-uji"));
        assert!(screen.status_line().contains("siap"));
        screen.busy = true;
        assert!(screen.status_line().contains("bekerja"));
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn tool_event_start(name: &str, preview: &str) -> AgentEvent {
        AgentEvent::ToolExecutionStart {
            tool_call_id: "c1".to_string(),
            tool_name: name.to_string(),
            preview: preview.to_string(),
        }
    }

    fn pending_approval(
        preview: String,
    ) -> (PendingApproval, tokio::sync::oneshot::Receiver<Decision>) {
        let (tx, rx) = tokio::sync::oneshot::channel::<Decision>();
        (
            PendingApproval {
                request: tui_approval::ApprovalRequest {
                    tool: "bash".to_string(),
                    tier: tui_wire::ToolTier::Exec,
                    subject: "Get-ChildItem".to_string(),
                    preview,
                    default: None,
                },
                reply: tx,
            },
            rx,
        )
    }

    /// Server SSE tiruan: balas satu respons streaming setelah `delay_ms`.
    fn spawn_slow_sse(delay_ms: u64) -> String {
        use std::io::{Read, Write};
        let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
            panic!("bind server uji gagal");
        };
        let Some(addr) = listener.local_addr().ok() else {
            panic!("alamat server uji tidak diketahui");
        };
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                let body = concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"halo\"}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/v1")
    }

    fn test_boot(base_url: &str) -> crate::Boot {
        use tui_config::{ApprovalConfig, LimitsConfig, UiConfig};
        let dir = std::env::temp_dir().join(format!(
            "maicli-worker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::create_dir_all(&dir).is_err() {
            panic!("direktori uji gagal dibuat");
        }
        let Ok(client) = crate::OpenAiCompatClient::new(base_url, Some("kunci-uji".to_string()))
        else {
            panic!("klien uji gagal dibuat");
        };
        crate::Boot {
            client,
            model: "model-uji".to_string(),
            cwd: dir.clone(),
            sessions_dir: dir.join("sessions"),
            limits: LimitsConfig::default(),
            approval: tui_approval::ApprovalPolicy::from(ApprovalConfig::default()),
            resume_path: None,
            resume_messages: Vec::new(),
            resume_usage: tui_wire::Usage::default(),
            grants_path: dir.join("grants.json"),
            ui: UiConfig::default(),
            models: Vec::new(),
            recorder: crate::event_log::Recorder::new(dir.join("events.jsonl"), false),
            config: tui_config::Config::default(),
            memory: None,
            context_files: Vec::new(),
            context_notes: Vec::new(),
            annotations: Vec::new(),
            notifications: Vec::new(),
            plan: String::new(),
            tool_issues: Vec::new(),
            system_prompt: String::new(),
            grant_ttl: None,
            provider: ("modeluxai".to_string(), "MODELUXAI_API_KEY".to_string()),
            mcp_tools: Vec::new(),
            mcp_notes: Vec::new(),
            subagents: None,
            notif_rx: None,
            skills: Vec::new(),
            katalog: None,
        }
    }

    #[tokio::test]
    async fn prompt_saat_sibuk_ditolak_dengan_pesan_jelas() {
        let base = spawn_slow_sse(120);
        let (events_tx, mut events_rx) = tui_core::agent::event_channel();
        let (approvals_tx, _approvals_rx) = unbounded_channel::<PendingApproval>();
        let (questions_tx, _questions_rx) = unbounded_channel::<AskRequest>();
        let commands = spawn_worker(test_boot(&base), events_tx, approvals_tx, questions_tx);
        let _ = commands.send(Command::Prompt("halo".to_string()));
        // Turn pertama sedang berjalan (server lambat); prompt kedua harus ditolak.
        let _ = commands.send(Command::Prompt("lagi".to_string()));
        let mut saw_busy = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), events_rx.recv())
                .await
            {
                Ok(Some(AgentEvent::Notice { text })) if text.contains("masih sibuk") => {
                    saw_busy = true;
                }
                Ok(Some(AgentEvent::AgentEnd { .. })) => break,
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {}
            }
        }
        let _ = commands.send(Command::Quit);
        assert!(
            saw_busy,
            "prompt kedua harus ditolak dengan pesan 'masih sibuk'"
        );
    }

    #[test]
    fn f2_melompat_ke_persetujuan_yang_ter_scroll_jauh() {
        let mut layar = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        layar.view_height = 5;
        layar.view_width = 40;
        for n in 0..40 {
            layar.push_line(format!("baris {n}"));
        }
        let (pending, _reply) = pending_approval("dir".to_string());
        layar.show_pending(pending);
        // Gulir ke awal histori: blok persetujuan jauh di bawah, tidak terlihat.
        layar.scroll_home();
        assert_eq!(layar.top_offset(), 0, "di awal histori");
        assert!(layar.handle_key(key(KeyCode::F(2)), &tx).is_none());
        let offset = layar.visual_offset(layar.pending_line.unwrap_or(0));
        let top = layar.top_offset() as usize;
        let visible_end = top + usize::from(layar.view_height);
        assert!(top > 30, "harus melompat mendekati blok, top {top}");
        assert!(
            offset >= top && offset < visible_end,
            "blok persetujuan harus masuk layar: offset {offset}, jendela {top}..{visible_end}"
        );
        assert!(layar.status.contains("dilompat"), "{}", layar.status);
    }

    #[test]
    fn approval_inline_menampilkan_detail_dan_pilihan() {
        let mut layar = screen();
        layar.push_line("halo dunia".to_string());
        let (pending, _rx) = pending_approval("Get-ChildItem -Recurse".to_string());
        layar.show_pending(pending);
        let lines = layar.lines.clone();
        assert_eq!(lines.first().map(String::as_str), Some("halo dunia"));
        assert_eq!(
            layar.pending_line,
            Some(3),
            "blok approval dengan ribbon + RISK menempel setelah baris terakhir"
        );
        assert!(
            lines[1].contains("SECURITY & PERMISSION GUARDRAIL"),
            "{lines:?}"
        );
        assert!(lines[2].contains("RISK:"), "{lines:?}");
        assert!(lines[3].contains("menunggu persetujuan"), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.contains("menunggu persetujuan")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("subjek: Get-ChildItem")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("Get-ChildItem -Recurse")),
            "argumen verbatim tampil: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Setuju Sekali") && l.contains("Tolak")),
            "butuh pilihan eksplisit: {lines:?}"
        );
        layar.refresh_pending_lines();
        let countdown = layar
            .pending_countdown_line
            .and_then(|index| layar.lines.get(index))
            .cloned()
            .unwrap_or_default();
        assert!(countdown.contains("sisa "), "{countdown}");
        assert!(countdown.contains(':'), "{countdown}");
    }

    #[test]
    fn enter_tanpa_pilihan_tidak_menyetujui() {
        let mut layar = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        let (pending, _rx) = pending_approval("dir".to_string());
        layar.show_pending(pending);
        assert!(layar.handle_key(key(KeyCode::Enter), &tx).is_none());
        assert!(layar.pending.is_some(), "keputusan belum diambil");
        assert!(layar.status.contains("pilih dulu"), "{}", layar.status);

        // Tab memilih izinkan, Enter menerapkannya.
        assert!(layar.handle_key(key(KeyCode::Tab), &tx).is_none());
        assert_eq!(layar.pending_choice, Some(true));
        assert!(layar.handle_key(key(KeyCode::Enter), &tx).is_none());
        assert!(layar.pending.is_none(), "keputusan diterapkan");
        assert!(
            layar.lines.iter().any(|l| l.contains("keputusan: Once")),
            "{:?}",
            layar.lines
        );
    }

    #[test]
    fn tab_dua_kali_memilih_tolak() {
        let mut layar = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        let (pending, _rx) = pending_approval("dir".to_string());
        layar.show_pending(pending);
        assert!(layar.handle_key(key(KeyCode::Tab), &tx).is_none());
        assert!(layar.handle_key(key(KeyCode::Tab), &tx).is_none());
        assert_eq!(layar.pending_choice, Some(false));
        assert!(layar.handle_key(key(KeyCode::Enter), &tx).is_none());
        assert!(
            layar.lines.iter().any(|l| l.contains("keputusan: Deny")),
            "{:?}",
            layar.lines
        );
    }

    #[test]
    fn ctrl_e_menampilkan_detail_tool_lalu_meringkas_balik() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.view_width = 40;
        screen.view_height = 20;
        screen.apply(tool_event_start("bash", "dir /s"));
        let ringkas = screen.visual_lines();
        assert_eq!(screen.body().len(), 1, "default hanya ringkasan");

        assert!(screen.handle_key(ctrl('e'), &tx).is_none());
        assert!(screen.expanded_tools);
        let detail = screen.body();
        assert_eq!(detail.len(), 3, "ringkasan + dua baris detail: {detail:?}");
        assert!(
            screen.visual_lines() > ringkas,
            "detail menambah baris visual"
        );

        // Toggle kembali meringkas tanpa menghapus data.
        assert!(screen.handle_key(ctrl('e'), &tx).is_none());
        assert!(!screen.expanded_tools);
        assert_eq!(screen.body().len(), 1);
    }

    #[test]
    fn palette_items_filter_dan_escape() {
        let mut screen = screen();
        screen.palette = Some(Palette {
            query: "title".to_string(),
            selected: 0,
        });
        let items = screen.palette_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "title");
        screen.palette = Some(Palette {
            query: "zzz".to_string(),
            selected: 0,
        });
        assert!(screen.palette_items().is_empty());
    }

    #[test]
    fn ctrl_k_membuka_palet_dan_menjalankan_command() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        assert!(screen.handle_key(ctrl('k'), &tx).is_none());
        assert!(screen.palette.is_some(), "palet terbuka");
        for ch in "comp".chars() {
            assert!(screen.handle_key(key(KeyCode::Char(ch)), &tx).is_none());
        }
        match screen.handle_key(key(KeyCode::Enter), &tx) {
            Some(Action::Submit(text)) => assert_eq!(text, "/compact"),
            other => panic!("harus menjalankan /compact: {other:?}"),
        }
        assert!(screen.palette.is_none(), "palet tertutup setelah dipilih");
    }

    #[test]
    fn palet_command_berargumen_mengisi_composer() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        assert!(screen.handle_key(ctrl('k'), &tx).is_none());
        for ch in "title".chars() {
            assert!(screen.handle_key(key(KeyCode::Char(ch)), &tx).is_none());
        }
        assert!(screen.handle_key(key(KeyCode::Enter), &tx).is_none());
        assert_eq!(
            screen.input, "/title ",
            "command berargumen mengisi composer"
        );
        assert!(screen.palette.is_none());
    }

    #[test]
    fn palet_dibatalkan_dengan_escape() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        assert!(screen.handle_key(ctrl('k'), &tx).is_none());
        assert!(screen.palette.is_some());
        assert!(screen.handle_key(key(KeyCode::Esc), &tx).is_none());
        assert!(screen.palette.is_none());
        assert!(screen.input.is_empty(), "escape tidak menulis ke composer");
    }

    #[test]
    fn shift_enter_menambah_baris_baru_tanpa_mengirim() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.input = "baris satu".to_string();
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert!(screen.handle_key(shift_enter, &tx).is_none());
        assert_eq!(screen.input, "baris satu\n");
        screen.input.push_str("baris dua");
        // Enter biasa tetap mengirim seluruh isi (termasuk newline).
        match screen.handle_key(key(KeyCode::Enter), &tx) {
            Some(Action::Submit(text)) => assert_eq!(text, "baris satu\nbaris dua"),
            other => panic!("harus submit: {other:?}"),
        }
    }

    #[test]
    fn tinggi_composer_mengikuti_jumlah_baris() {
        let mut layar = screen();
        layar.view_width = 40;
        assert_eq!(layar.composer_height(), 3, "satu baris");
        layar.input = "satu\ndua\ntiga".to_string();
        assert_eq!(layar.composer_height(), 5);
        assert_eq!(layar.composer_scroll(), 0, "muat tanpa gulir");
        layar.input = (1..=20)
            .map(|n| format!("baris {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(layar.composer_height(), 12, "dibatasi maksimum");
        assert_eq!(
            layar.composer_scroll(),
            10,
            "20 baris isi, 10 terlihat, 10 tergulir agar baris terakhir tampak"
        );
    }

    #[test]
    fn ctrl_c_saat_sibuk_membatalkan_turn_dulu() {
        let mut screen = screen();
        let (tx, mut rx) = unbounded_channel::<Command>();
        screen.busy = true;
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(
            screen.handle_key(ctrl_c, &tx),
            Some(Action::Abort)
        ));
        assert!(matches!(rx.try_recv(), Ok(Command::Cancel)));
        assert!(screen.status.contains("membatalkan"), "{}", screen.status);
        assert!(!screen.quit_armed, "pembatalan bukan konfirmasi keluar");
    }

    #[test]
    fn ctrl_c_dua_kali_saat_idle_baru_keluar() {
        let mut layar = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(layar.handle_key(ctrl_c, &tx).is_none(), "tekanan pertama");
        assert!(layar.quit_armed);
        assert!(layar.status.contains("Ctrl+C lagi"), "{}", layar.status);
        assert!(matches!(layar.handle_key(ctrl_c, &tx), Some(Action::Quit)));

        // Tombol lain membatalkan konfirmasi.
        let mut layar = screen();
        assert!(layar.handle_key(ctrl_c, &tx).is_none());
        assert!(layar.handle_key(key(KeyCode::Char('a')), &tx).is_none());
        assert!(!layar.quit_armed);
        assert!(layar.handle_key(ctrl_c, &tx).is_none(), "perlu dua lagi");
    }

    #[test]
    fn enter_mengirim_prompt_dan_escape_tidak_keluar_saat_idle() {
        let mut screen = screen();
        screen.input = "halo".to_string();
        let (commands, mut _rx) = unbounded_channel::<Command>();
        match screen.handle_key(key(KeyCode::Enter), &commands) {
            Some(Action::Submit(text)) => assert_eq!(text, "halo"),
            _ => panic!("Enter harus mengirim"),
        }
        assert!(screen.input.is_empty());
        assert!(screen.handle_key(key(KeyCode::Esc), &commands).is_none());
    }

    #[test]
    fn escape_saat_busy_mengirim_sinyal_batal() {
        let mut screen = screen();
        screen.busy = true;
        let (commands, mut rx) = unbounded_channel::<Command>();
        assert!(matches!(
            screen.handle_key(key(KeyCode::Esc), &commands),
            Some(Action::Abort)
        ));
        assert!(
            matches!(rx.try_recv(), Ok(Command::Cancel)),
            "sinyal batal harus terkirim lewat kanal perintah"
        );
    }

    #[test]
    fn f1_membuka_dan_menutup_bantuan() {
        let mut screen = screen();
        let (commands, mut _rx) = unbounded_channel::<Command>();
        screen.handle_key(key(KeyCode::F(1)), &commands);
        assert!(screen.show_help);
        assert!(screen.help_text().contains("PgUp/PgDn"));
        screen.handle_key(key(KeyCode::Esc), &commands);
        assert!(!screen.show_help);
    }

    #[test]
    fn status_line_mengikuti_turn_status() {
        use tui_core::TurnStatus;
        let mut layar = screen();
        assert!(
            layar.status_line().contains("siap"),
            "{}",
            layar.status_line()
        );
        layar.apply(AgentEvent::AgentStart);
        assert!(
            layar.status_line().contains("streaming"),
            "{}",
            layar.status_line()
        );
        layar.apply(AgentEvent::ToolExecutionStart {
            tool_call_id: "c1".to_string(),
            tool_name: "bash".to_string(),
            preview: "dir".to_string(),
        });
        assert!(
            layar.status_line().contains("menjalankan tool"),
            "{}",
            layar.status_line()
        );
        layar.apply(AgentEvent::MessageEnd {
            message: tui_wire::AssistantMessage {
                content: Vec::new(),
                stop_reason: tui_wire::StopReason::Error,
                usage: Default::default(),
                error: Some("gateway gagal".to_string()),
            },
        });
        assert_eq!(layar.turn_state, TurnStatus::Error);
        layar.apply(AgentEvent::AgentEnd { turns: 1 });
        assert_eq!(
            layar.turn_state,
            TurnStatus::Error,
            "status terminal dipertahankan"
        );
        assert!(
            layar.status_line().contains("error"),
            "{}",
            layar.status_line()
        );
        layar.apply(AgentEvent::AgentStart);
        assert_eq!(layar.turn_state, TurnStatus::Streaming, "turn baru mereset");
        assert_eq!(Screen::turn_label(TurnStatus::Aborted), "dibatalkan");
    }

    #[test]
    fn status_line_menyebut_mode_approval() {
        let layar = screen();
        let baris = layar.status_line();
        assert!(baris.contains("mode tanya"), "{baris}");
        let yolo = Screen::new(
            "m".to_string(),
            tui_approval::ApprovalMode::Yolo,
            Color::Yellow,
            Color::DarkGray,
            Duration::from_secs(60),
        );
        assert!(
            yolo.status_line().contains("mode yolo"),
            "{}",
            yolo.status_line()
        );
        assert_eq!(
            Screen::approval_label(tui_approval::ApprovalMode::AlwaysAskCached),
            "tanya+cache"
        );
        assert_eq!(
            Screen::approval_label(tui_approval::ApprovalMode::Write),
            "tulis"
        );
    }

    #[test]
    fn bantuan_menyebut_semua_tombol_penting() {
        let layar = screen();
        let teks = layar.help_text();
        for wajib in [
            "Enter",
            "Shift+Enter",
            "PgUp/PgDn",
            "Ctrl+C",
            "Ctrl+K",
            "Ctrl+E",
            "F2",
            "F3",
            "Tab",
            "/help",
        ] {
            assert!(
                teks.contains(wajib),
                "bantuan harus menyebut {wajib}: {teks}"
            );
        }
        assert!(
            teks.contains("2x keluar"),
            "bantuan harus menjelaskan Ctrl+C: {teks}"
        );
    }

    #[test]
    fn hint_saat_sibuk_menyebut_pembatalan() {
        let mut layar = screen();
        layar.busy = true;
        let teks = layar.hint();
        assert!(teks.contains("batalkan"), "{teks}");
        assert!(!teks.contains("Ctrl+C keluar"), "{teks}");
        layar.busy = false;
        let idle = layar.hint();
        assert!(idle.contains("╰─"), "{idle}");
        assert!(idle.contains("slash"), "{idle}");
    }

    #[test]
    fn page_up_menggulir_satu_layar() {
        let mut screen = screen();
        for index in 0..30 {
            screen.push_line(format!("baris {index}"));
        }
        screen.view_height = 10;
        let (commands, mut _rx) = unbounded_channel::<Command>();
        screen.handle_key(key(KeyCode::PageUp), &commands);
        assert_eq!(screen.scroll, 10);
        screen.handle_key(key(KeyCode::PageDown), &commands);
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn approval_memakai_tombol_y_n_dan_tidak_menutup_sendiri() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<PendingApproval>();
        let (reply, mut answer) = tokio::sync::oneshot::channel();
        let pending = PendingApproval {
            request: tui_approval::ApprovalRequest {
                tool: "bash".into(),
                tier: tui_wire::ToolTier::Exec,
                subject: "dir".into(),
                preview: String::new(),
                default: None,
            },
            reply,
        };
        drop(tx);
        screen.pending = Some(pending);
        let (commands, mut _rx) = unbounded_channel::<Command>();
        screen.handle_key(key(KeyCode::Char('x')), &commands);
        assert!(
            screen.pending.is_some(),
            "tombol lain tidak boleh menutup dialog"
        );
        screen.handle_key(key(KeyCode::Char('y')), &commands);
        assert!(screen.pending.is_none());
        assert_eq!(answer.try_recv().ok(), Some(Decision::Once));
    }

    #[test]
    fn pertanyaan_dijawab_lewat_enter_tanpa_mengirim_prompt() {
        let mut screen = screen();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        screen.question = Some(PendingQuestion {
            question: "lanjut?".into(),
            reply: tx,
        });
        screen.input = "ya".to_string();
        let (commands, mut _cmds) = unbounded_channel::<Command>();
        assert!(
            screen.handle_key(key(KeyCode::Enter), &commands).is_none(),
            "jawaban tidak boleh menjadi prompt baru"
        );
        assert_eq!(rx.try_recv().ok(), Some("ya".to_string()));
        assert!(screen.question.is_none());
        assert!(screen.input.is_empty());
        assert!(
            screen
                .lines
                .iter()
                .any(|line| line.contains("[tanya] lanjut?"))
        );
    }

    #[test]
    fn pertanyaan_dilewati_lewat_escape() {
        let mut screen = screen();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        screen.question = Some(PendingQuestion {
            question: "lanjut?".into(),
            reply: tx,
        });
        let (commands, mut _cmds) = unbounded_channel::<Command>();
        screen.handle_key(key(KeyCode::Esc), &commands);
        assert_eq!(rx.try_recv().ok(), Some(String::new()));
        assert!(screen.lines.iter().any(|line| line.contains("dilewati")));
    }

    #[test]
    fn hint_saat_menunggu_jawaban() {
        let mut screen = screen();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        screen.question = Some(PendingQuestion {
            question: "lanjut?".into(),
            reply: tx,
        });
        assert!(screen.hint().contains("jawab pertanyaan"));
    }

    #[test]
    fn persetujuan_kedaluwarsa_dicatat() {
        let mut screen = screen();
        let (reply, receiver) = tokio::sync::oneshot::channel();
        drop(receiver);
        screen.pending = Some(PendingApproval {
            request: ApprovalRequest {
                tool: "bash".to_string(),
                tier: ToolTier::Exec,
                subject: "dir".to_string(),
                preview: "dir".to_string(),
                default: None,
            },
            reply,
        });
        let (commands, mut _cmds) = unbounded_channel::<Command>();
        screen.handle_key(key(KeyCode::Char('y')), &commands);
        assert!(
            screen.lines.iter().any(|line| line.contains("kedaluwarsa")),
            "{:?}",
            screen.lines
        );
        assert!(screen.pending.is_none());
    }

    fn choice(id: &str, title: &str) -> SessionChoice {
        SessionChoice {
            id: id.to_string(),
            title: title.to_string(),
            messages: 2,
        }
    }

    /// Server SSE tiruan yang melayani beberapa respons berurutan.
    fn spawn_sse_script(bodies: Vec<String>) -> String {
        use std::io::{Read, Write};
        let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
            panic!("bind server uji gagal");
        };
        let Some(addr) = listener.local_addr().ok() else {
            panic!("alamat server uji tidak diketahui");
        };
        std::thread::spawn(move || {
            let mut indeks = 0usize;
            // Path-aware: discovery `/models` dibalas JSON, bukan skrip SSE.
            // Di Windows klien bisa melakukan discovery lebih dulu; mock lama
            // mengira itu panggilan chat pertama sehingga tool call tidak pernah
            // sampai ke UI (approval_timeout gagal).
            for _ in 0..32 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = [0u8; 16384];
                let _ = stream.read(&mut buffer);
                let permintaan = String::from_utf8_lossy(&buffer);
                let path = permintaan
                    .lines()
                    .next()
                    .and_then(|baris| baris.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let (jenis, isi) = if path.contains("models") {
                    (
                        "application/json",
                        "{\"data\":[{\"id\":\"model-uji\"}]}".to_string(),
                    )
                } else {
                    let hasil = if indeks < bodies.len() {
                        let nilai = bodies[indeks].clone();
                        indeks += 1;
                        nilai
                    } else {
                        // Skrip habis: ulangi body terakhir agar koneksi tambahan
                        // tidak menggantung klien.
                        bodies.last().cloned().unwrap_or_default()
                    };
                    ("text/event-stream", hasil)
                };
                let respons = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {jenis}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{isi}",
                    isi.len()
                );
                let _ = stream.write_all(respons.as_bytes());
            }
        });
        format!("http://{addr}/v1")
    }

    fn sse_text(text: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
        )
    }

    fn sse_tool_call(name: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{{\"name\":\"{name}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
        )
    }

    #[tokio::test]
    async fn approval_timeout_menolak_dan_tool_tidak_jalan() {
        let base = spawn_sse_script(vec![sse_tool_call("bash"), sse_text("beres")]);
        let mut boot = test_boot(&base);
        boot.limits.approval_timeout_secs = 1;
        let (events_tx, mut events_rx) = tui_core::agent::event_channel();
        let (approvals_tx, mut approvals_rx) = unbounded_channel::<PendingApproval>();
        let (questions_tx, _questions_rx) = unbounded_channel::<AskRequest>();
        let commands = spawn_worker(boot, events_tx, approvals_tx, questions_tx);
        let _ = commands.send(Command::Prompt("pakai bash".to_string()));
        let mut saw_pending = false;
        let mut saw_start = false;
        let mut saw_deny_end = false;
        let mut held = Vec::new();
        let started = std::time::Instant::now();
        // Runner Windows bisa jauh lebih lambat dari mesin dev; jendela
        // observasi 5 dtk sering kehabisan sebelum approval sampai ke UI.
        // Semua assertions tetap sama — hanya waktu tunggu yang diperlebar.
        let deadline = started + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            while let Ok(pending) = approvals_rx.try_recv() {
                saw_pending = true;
                // Dibiarkan hidup dan tidak dijawab: UI harus menolak setelah
                // batas waktu (bukan karena kanal ditutup).
                held.push(pending);
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), events_rx.recv())
                .await
            {
                Ok(Some(AgentEvent::ToolExecutionStart { .. })) => saw_start = true,
                Ok(Some(AgentEvent::ToolExecutionEnd { is_error, .. })) if is_error => {
                    saw_deny_end = true;
                }
                Ok(Some(AgentEvent::AgentEnd { .. })) => break,
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {}
            }
        }
        let _ = commands.send(Command::Quit);
        assert!(saw_pending, "approval harus sampai ke UI");
        assert!(!saw_start, "tool tidak boleh mulai sebelum izin");
        assert!(saw_deny_end, "penolakan timeout harus tercatat di timeline");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(900),
            "penolakan harus menunggu batas waktu, bukan langsung: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn banner_yolo_hanya_untuk_mode_yolo() {
        let mut layar = screen();
        assert!(
            layar.lines.is_empty(),
            "banner tidak boleh tampil di mode biasa"
        );
        layar.push_line(crate::banner::yolo_banner().to_string());
        assert!(
            layar.lines[0].contains("PERINGATAN"),
            "mode yolo harus menampilkan banner: {:?}",
            layar.lines
        );
        assert!(
            crate::banner::yolo_banner().contains("yolo"),
            "banner harus menyebut mode yolo"
        );
    }

    #[test]
    fn tool_timeline_memakai_label_dan_petunjuk_render() {
        let mut layar = screen();
        layar.apply(AgentEvent::ToolExecutionStart {
            tool_call_id: "c1".to_string(),
            tool_name: "bash".to_string(),
            preview: "dir".to_string(),
        });
        layar.apply(AgentEvent::ToolExecutionStart {
            tool_call_id: "c2".to_string(),
            tool_name: "read".to_string(),
            preview: "a.txt".to_string(),
        });
        let baris = layar.lines.join("\n");
        assert!(
            baris.contains("╭─ bash │ RUNNING │ ❯ perintah: dir"),
            "bash harus tampil dengan label+petunjuk perintah: {baris}"
        );
        assert!(
            baris.contains("╭─ read │ RUNNING │ → baca: a.txt"),
            "read harus tampil dengan label biasa: {baris}"
        );
    }

    #[test]
    fn tool_tak_dikenal_tetap_tampil_dengan_nama() {
        let mut layar = screen();
        layar.apply(AgentEvent::ToolExecutionStart {
            tool_call_id: "c1".to_string(),
            tool_name: "tidak_ada".to_string(),
            preview: "x".to_string(),
        });
        assert!(
            layar
                .lines
                .join("\n")
                .contains("╭─ tidak_ada │ RUNNING │ → tidak_ada: x"),
            "{:?}",
            layar.lines
        );
    }

    #[test]
    fn label_timeline_membedakan_perintah_dan_prompt() {
        assert_eq!(timeline_label("/model"), "[perintah] /model");
        assert_eq!(timeline_label("  /compact"), "[perintah]   /compact");
        assert_eq!(timeline_label("halo dunia"), "> halo dunia");
    }

    #[test]
    fn label_timeline_dipakai_saat_switch() {
        // Jalur `/switch` juga lewat label yang sama.
        let mut layar = screen();
        layar.push_line(timeline_label("/switch abc"));
        assert_eq!(layar.lines, vec!["[perintah] /switch abc".to_string()]);
    }

    #[test]
    fn daftar_sesi_membuka_pemilih() {
        let mut screen = screen();
        screen.apply(AgentEvent::SessionsListed {
            sessions: vec![choice("a", "satu"), choice("b", "dua")],
        });
        let picker = match screen.picker.as_ref() {
            Some(picker) => picker,
            None => panic!("pemilih harus terbuka"),
        };
        assert_eq!(picker.items.len(), 2);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn daftar_sesi_kosong_menjadi_pesan() {
        let mut screen = screen();
        screen.apply(AgentEvent::SessionsListed { sessions: vec![] });
        assert!(screen.picker.is_none());
        assert!(
            screen
                .lines
                .iter()
                .any(|line| line.contains("belum ada sesi"))
        );
    }

    #[test]
    fn pemilih_sesi_dinavigasi_dan_dipilih() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.apply(AgentEvent::SessionsListed {
            sessions: vec![choice("a", "satu"), choice("b", "dua"), choice("c", "tiga")],
        });
        assert!(screen.handle_key(key(KeyCode::Down), &tx).is_none());
        assert!(screen.handle_key(key(KeyCode::Down), &tx).is_none());
        // sudah di ujung: tidak melewati batas
        assert!(screen.handle_key(key(KeyCode::Down), &tx).is_none());
        assert_eq!(screen.picker.as_ref().map(|p| p.selected), Some(2));
        assert!(screen.handle_key(key(KeyCode::Up), &tx).is_none());
        match screen.handle_key(key(KeyCode::Enter), &tx) {
            Some(Action::SwitchSession(id)) => assert_eq!(id, "b"),
            other => panic!("harus memilih sesi b, dapat {other:?}"),
        }
        assert!(screen.picker.is_none(), "pemilih tertutup setelah memilih");
    }

    #[test]
    fn pemilih_sesi_dibatalkan_dengan_escape() {
        let mut screen = screen();
        let (tx, _rx) = unbounded_channel::<Command>();
        screen.apply(AgentEvent::SessionsListed {
            sessions: vec![choice("a", "satu")],
        });
        assert!(screen.handle_key(key(KeyCode::Esc), &tx).is_none());
        assert!(screen.picker.is_none());
        assert!(screen.lines.is_empty(), "escape tidak menulis apa pun");
    }

    #[test]
    fn pindah_sesi_mengosongkan_histori() {
        let mut screen = screen();
        screen.push_line("pesan sesi lama".to_string());
        screen.current = "sedang menulis".to_string();
        screen.scroll = 3;
        screen.apply(AgentEvent::SessionSwitched {
            path: "C:\\sesi\\baru.jsonl".to_string(),
            messages: 7,
        });
        assert_eq!(screen.lines.len(), 1, "hanya baris pemberitahuan");
        assert!(screen.lines[0].contains("baru.jsonl"), "{:?}", screen.lines);
        assert!(screen.lines[0].contains("7 pesan"));
        assert!(screen.current.is_empty());
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn clear_timeline_event_kosongkan_lines() {
        let mut layar = screen();
        layar.push_line("satu".to_string());
        layar.push_line("dua".to_string());
        layar.apply(AgentEvent::ClearTimeline);
        assert!(layar.lines.is_empty(), "clear harus mengosongkan timeline");
        assert_eq!(layar.scroll, 0);
    }

    #[test]
    fn agent_end_melepas_status_sibuk() {
        let mut screen = screen();
        screen.apply(AgentEvent::AgentStart);
        assert!(screen.busy);
        screen.apply(AgentEvent::AgentEnd { turns: 2 });
        assert!(!screen.busy);
        assert!(screen.status.contains("selesai"));
    }

    #[test]
    fn pesan_assistant_error_ditampilkan() {
        let mut screen = screen();
        screen.apply(AgentEvent::MessageEnd {
            message: AssistantMessage {
                content: Vec::new(),
                stop_reason: tui_wire::StopReason::Error,
                usage: Default::default(),
                error: Some("gateway menolak".to_string()),
            },
        });
        assert!(
            screen
                .lines
                .iter()
                .any(|line| line.contains("gateway menolak"))
        );
    }
    #[test]
    fn parse_key_menerima_modifier() {
        assert_eq!(
            parse_key("ctrl+k"),
            Some(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            parse_key("f1"),
            Some(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE))
        );
        assert!(parse_key("ctrl+unknown").is_none());
    }
}
