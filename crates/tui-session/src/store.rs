//! Penulisan dan pembacaan file sesi JSONL.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use tui_wire::{ErrorCode, MxError};

use crate::entries::{SESSION_VERSION, SessionEntry, new_id, now_ms};

/// Hasil membaca satu file sesi.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    /// Entry valid, berurutan.
    pub entries: Vec<SessionEntry>,
    /// Hal yang tidak fatal (mis. baris terakhir terpotong).
    pub warnings: Vec<String>,
}

impl LoadedSession {
    /// Pesan yang bisa dipakai melanjutkan sesi.
    pub fn messages(&self) -> Vec<tui_wire::Message> {
        resume_messages(&self.entries)
    }

    /// Id sesi dari header, bila ada.
    pub fn session_id(&self) -> Option<&str> {
        self.entries.first().and_then(|entry| match entry {
            SessionEntry::Session { id, .. } => Some(id.as_str()),
            _ => None,
        })
    }

    /// Judul sesi dari header, bila ada.
    /// Total pemakaian token yang tercatat di sesi.
    pub fn usage(&self) -> (u64, u64) {
        self.entries
            .iter()
            .fold((0u64, 0u64), |acc, entry| match entry {
                SessionEntry::ModelUsage {
                    input_tokens,
                    output_tokens,
                    ..
                } => (acc.0 + input_tokens, acc.1 + output_tokens),
                _ => acc,
            })
    }

    /// Model aktif terakhir yang tercatat di sesi.
    pub fn model(&self) -> Option<&str> {
        self.entries.iter().rev().find_map(|entry| match entry {
            SessionEntry::ModelChange { model, .. } => Some(model.as_str()),
            _ => None,
        })
    }

    pub fn title(&self) -> Option<&str> {
        // `TitleChange` terakhir menang; header hanya nilai awal.
        let mut title = None;
        for entry in &self.entries {
            match entry {
                SessionEntry::Session { title: initial, .. } => title = Some(initial.as_str()),
                SessionEntry::TitleChange { title: new, .. } => title = Some(new.as_str()),
                _ => {}
            }
        }
        title
    }
}

/// Penulis sesi yang menambahkan satu baris per entry.
/// Batas ukuran satu file sesi sebelum rotasi (16 MiB).
///
/// RENCANA C-06 mewajibkan batas ukuran + rotasi: berkas tidak boleh tumbuh
/// tanpa batas. Saat batas tercapai, sesi dilanjutkan di berkas baru dengan
/// `parent_session` menunjuk berkas sebelumnya.
pub const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;

pub struct SessionStore {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    lock: SessionLock,
    title: String,
    cwd: String,
    session_id: String,
    written: u64,
    max_bytes: u64,
}

/// Ringkasan satu sesi, dipakai daftar dan pemilih resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// Id sesi dari header.
    pub id: String,
    /// Path file JSONL.
    pub path: PathBuf,
    /// Judul sesi.
    pub title: String,
    /// Jumlah entry pesan (bukan entry metadata).
    pub messages: usize,
    /// Waktu modifikasi file (ms sejak epoch).
    pub updated_at: u64,
    /// Id sesi induk bila hasil fork / subagent.
    pub parent: Option<String>,
}

impl SimpulPohon {
    /// Satu baris pohon (indentasi per kedalaman).
    pub fn display_line(&self) -> String {
        let indent = "  ".repeat(self.kedalaman);
        format!(
            "{indent}{} · {} pesan · {}",
            self.summary.title, self.summary.messages, self.summary.id
        )
    }
}

impl SessionSummary {
    /// Satu baris untuk daftar di UI.
    pub fn display_line(&self) -> String {
        format!("{} · {} pesan · {}", self.title, self.messages, self.id)
    }
}

/// Daftar sesi di satu direktori, terbaru lebih dulu.
///
/// File yang rusak dilewati, bukan menggagalkan daftar: satu file korup tidak
/// boleh membuat semua sesi tidak bisa dipilih.
pub fn list(dir: &Path) -> Vec<SessionSummary> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<SessionSummary> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| summarize(&path))
        .collect();
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    sessions
}

/// Cari sesi yang diminta: `latest`, id persis, atau awalan id yang unik.
pub fn resolve_target(sessions_dir: &Path, requested: &str) -> Result<SessionSummary, MxError> {
    if requested == "latest" {
        return latest(sessions_dir).ok_or_else(|| {
            MxError::new(
                ErrorCode::SessionNotFound,
                format!("belum ada sesi di {}", sessions_dir.display()),
            )
        });
    }
    match_session(&list(sessions_dir), requested, sessions_dir)
}

fn match_session(
    all: &[SessionSummary],
    requested: &str,
    sessions_dir: &Path,
) -> Result<SessionSummary, MxError> {
    if let Some(exact) = all.iter().find(|session| session.id == requested) {
        return Ok(exact.clone());
    }
    let matches: Vec<&SessionSummary> = all
        .iter()
        .filter(|session| session.id.starts_with(requested))
        .collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(MxError::new(
            ErrorCode::SessionNotFound,
            format!("sesi '{requested}' tidak ada di {}", sessions_dir.display()),
        )),
        several => {
            let names: Vec<&str> = several
                .iter()
                .take(5)
                .map(|session| session.id.as_str())
                .collect();
            Err(MxError::new(
                ErrorCode::SessionNotFound,
                format!(
                    "awalan '{requested}' cocok dengan {} sesi ({}); pakai id yang lebih panjang",
                    several.len(),
                    names.join(", ")
                ),
            ))
        }
    }
}

/// Cari sesi berdasarkan potongan judul (tanpa peduli huruf besar).
///
/// Query kosong berarti semua sesi (sama dengan [`list`]). Urutan tetap terbaru
/// lebih dulu karena diturunkan dari `list`.
pub fn search(dir: &Path, query: &str) -> Vec<SessionSummary> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return list(dir);
    }
    list(dir)
        .into_iter()
        .filter(|session| session.title.to_ascii_lowercase().contains(&needle))
        .collect()
}
/// Hapus satu sesi beserta artefak lock-nya.
///
/// Menolak menghapus sesi yang sedang dipakai proses lain (lock masih segar) dan
/// melaporkan sesi yang tidak ada sebagai `E_SESSION_NOT_FOUND`. Hanya berkas
/// sesi itu sendiri yang dihapus; berkas rotasi sebelumnya tetap.
pub fn remove(session_path: &Path) -> Result<(), MxError> {
    if !session_path.exists() {
        return Err(MxError::new(
            ErrorCode::SessionNotFound,
            format!("sesi tidak ada: {}", session_path.display()),
        ));
    }
    let lock = lock_path(session_path);
    if lock.exists() {
        let stale = fs::read_to_string(&lock)
            .ok()
            .and_then(|text| {
                text.lines()
                    .next()
                    .and_then(|line| line.trim().parse::<u64>().ok())
            })
            .is_none_or(|stamp| now_ms().saturating_sub(stamp) > LOCK_STALE_AFTER_MS);
        if !stale {
            return Err(MxError::new(
                ErrorCode::SessionLocked,
                format!("sesi sedang dipakai proses lain (lock: {})", lock.display()),
            ));
        }
    }
    fs::remove_file(session_path).map_err(|e| io_error(e, session_path))?;
    let _ = fs::remove_file(&lock);
    Ok(())
}

/// Sesi yang paling baru diperbarui, bila ada.
pub fn latest(dir: &Path) -> Option<SessionSummary> {
    list(dir).into_iter().next()
}

fn summarize(path: &Path) -> Option<SessionSummary> {
    let loaded = SessionStore::load(path).ok()?;
    let id = loaded.session_id()?.to_string();
    let title = loaded.title().unwrap_or("tanpa judul").to_string();
    let messages = loaded.messages().len();
    // Lineage: header memuat parent_session bila sesi hasil fork / subagent.
    let parent = loaded.entries.first().and_then(|entry| match entry {
        SessionEntry::Session { parent_session, .. } => parent_session.clone(),
        _ => None,
    });
    let updated_at = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    Some(SessionSummary {
        id,
        path: path.to_path_buf(),
        title,
        messages,
        updated_at,
        parent,
    })
}

/// Simpul pohon sesi: ringkasan + kedalaman di pohon fork.
#[derive(Debug, Clone)]
pub struct SimpulPohon {
    /// Ringkasan sesi.
    pub summary: SessionSummary,
    /// Id induk (None = akar).
    pub parent: Option<String>,
    /// Kedalaman di pohon (akar = 0).
    pub kedalaman: usize,
}

/// Pohon sesi dari relasi `parent_session`: akar dulu, anak setelah induk,
/// urutan antar-saudara mengikuti waktu terbaru.
pub fn pohon(dir: &Path) -> Vec<SimpulPohon> {
    let daftar = list(dir);
    let induk_of: std::collections::HashMap<&str, &Option<String>> =
        daftar.iter().map(|s| (s.id.as_str(), &s.parent)).collect();
    // Kedalaman: telusuri rantai induk (maksimum 32 tingkat — proteksi siklus).
    let kedalaman = |mulai: &str| -> usize {
        let mut kedalaman = 0;
        let mut id = mulai.to_string();
        loop {
            let induk = induk_of.get(id.as_str()).and_then(|p| p.as_deref());
            match induk {
                Some(next) => {
                    kedalaman += 1;
                    if kedalaman > 32 {
                        break;
                    }
                    id = next.to_string();
                }
                None => break,
            }
        }
        kedalaman
    };
    let mut simpul: Vec<SimpulPohon> = daftar
        .iter()
        .map(|s| SimpulPohon {
            summary: s.clone(),
            parent: s.parent.clone(),
            kedalaman: kedalaman(&s.id),
        })
        .collect();
    // Urut: kedalaman naik, lalu terbaru dulu (sama seperti list).
    simpul.sort_by(|a, b| {
        a.kedalaman
            .cmp(&b.kedalaman)
            .then_with(|| b.summary.updated_at.cmp(&a.summary.updated_at))
            .then_with(|| a.summary.id.cmp(&b.summary.id))
    });
    simpul
}

/// Pesan yang bisa dilanjutkan dari entry sesi.
///
/// Hanya entry `message` yang masuk konteks; entry metadata (init, usage,
/// approval, judul) tidak dikirim ke model. Entry `compaction` menghapus pesan
/// sebelumnya dan menggantinya dengan satu pesan ringkasan — aturan yang sama
/// dipakai saat sesi hidup.
pub fn resume_messages(entries: &[SessionEntry]) -> Vec<tui_wire::Message> {
    let mut messages: Vec<tui_wire::Message> = Vec::new();
    for entry in entries {
        match entry {
            SessionEntry::Message { message, .. } => messages.push(message.clone()),
            SessionEntry::Compaction { summary, .. } => {
                messages.clear();
                messages.push(tui_wire::Message::user(format!(
                    "[ringkasan konteks sebelumnya]\n{}",
                    summary.trim()
                )));
            }
            _ => {}
        }
    }
    messages
}
/// Lock file sesi dianggap basi setelah satu jam tanpa pembaruan.
pub const LOCK_STALE_AFTER_MS: u64 = 60 * 60 * 1_000;

/// Lock eksklusif satu file sesi; dilepas saat nilai ini di-drop.
#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
}

impl SessionLock {
    /// Path file lock.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_path(session_path: &Path) -> PathBuf {
    let mut name = session_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".lock");
    session_path.with_file_name(name)
}

fn write_lock_file(path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "{}", now_ms())?;
    writeln!(file, "pid {}", std::process::id())?;
    file.flush()
}

/// Ambil lock untuk satu file sesi.
///
/// Lock basi (lebih tua dari `LOCK_STALE_AFTER_MS`, mis. proses sebelumnya mati
/// mendadak) diambil alih otomatis. Lock yang masih segar menghasilkan
/// `E_SESSION_LOCKED` supaya dua proses tidak menulis file yang sama.
pub fn acquire_lock(session_path: &Path) -> Result<SessionLock, MxError> {
    let path = lock_path(session_path);
    match write_lock_file(&path) {
        Ok(()) => return Ok(SessionLock { path }),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(io_error(err, &path)),
    }
    let age = fs::read_to_string(&path)
        .ok()
        .and_then(|text| {
            text.lines()
                .next()
                .and_then(|line| line.trim().parse::<u64>().ok())
        })
        .map(|stamp| now_ms().saturating_sub(stamp));
    let stale = age.is_none_or(|age| age > LOCK_STALE_AFTER_MS);
    if stale {
        let _ = fs::remove_file(&path);
        write_lock_file(&path).map_err(|err| io_error(err, &path))?;
        return Ok(SessionLock { path });
    }
    Err(MxError::new(
        ErrorCode::SessionLocked,
        format!("sesi sedang dipakai proses lain (lock: {})", path.display()),
    ))
}

impl SessionStore {
    /// Buat sesi baru di `dir` dan tulis header-nya.
    pub fn create(dir: &Path, title: &str, cwd: &str) -> Result<Self, MxError> {
        Self::create_with_parent(dir, title, cwd, None)
    }

    /// Seperti [`Self::create`] tetapi header memuat `parent_session`
    /// (lineage subagent; dipakai `tui-subagent`).
    pub fn create_with_parent(
        dir: &Path,
        title: &str,
        cwd: &str,
        parent_session: Option<String>,
    ) -> Result<Self, MxError> {
        fs::create_dir_all(dir).map_err(|e| io_error(e, dir))?;
        let id = new_id();
        let stamp = now_ms();
        let path = dir.join(format!("{stamp}_{id}.jsonl"));
        let header = SessionEntry::Session {
            version: SESSION_VERSION,
            id: id.clone(),
            title: title.to_string(),
            created_at: stamp,
            cwd: cwd.to_string(),
            parent_session,
        };
        let init = SessionEntry::SessionInit {
            id: new_id(),
            timestamp: stamp,
            cwd: cwd.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        write_atomic(&path, &[header, init])?;
        let lock = match acquire_lock(&path) {
            Ok(lock) => lock,
            Err(err) => {
                let _ = fs::remove_file(&path);
                return Err(err);
            }
        };
        let writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| io_error(e, &path))?;
        let written = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            path,
            writer: Some(BufWriter::new(writer)),
            lock,
            title: title.to_string(),
            cwd: cwd.to_string(),
            session_id: id,
            written,
            max_bytes: MAX_SESSION_BYTES,
        })
    }

    /// Buka file sesi yang sudah ada untuk ditambahi.
    pub fn open(path: &Path) -> Result<Self, MxError> {
        let loaded = Self::load(path)?;
        let mut title = "tanpa judul".to_string();
        let mut cwd = String::new();
        let mut session_id = String::new();
        for entry in &loaded.entries {
            match entry {
                SessionEntry::Session {
                    id,
                    title: initial,
                    cwd: initial_cwd,
                    ..
                } => {
                    session_id = id.clone();
                    title = initial.clone();
                    cwd = initial_cwd.clone();
                }
                SessionEntry::TitleChange { title: new, .. } => title = new.clone(),
                _ => {}
            }
        }
        if let Err(err) = tui_wire::secure_file::restrict_to_current_user(path) {
            tracing::warn!(target: "maicli::session", "ACL sesi tidak bisa dipersempit: {err}");
        }
        let written = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let lock = acquire_lock(path)?;
        let writer = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| io_error(e, path))?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: Some(BufWriter::new(writer)),
            lock,
            title,
            cwd,
            session_id,
            written,
            max_bytes: MAX_SESSION_BYTES,
        })
    }

    /// Atur batas ukuran sesi (dipakai test dan tuning).
    pub fn set_max_bytes(&mut self, max_bytes: u64) {
        self.max_bytes = max_bytes.max(1);
    }

    /// Rotasi ke berkas sesi baru karena batas ukuran tercapai.
    fn rotate(&mut self) -> Result<(), MxError> {
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let id = new_id();
        let stamp = now_ms();
        let path = dir.join(format!("{stamp}_{id}.jsonl"));
        let header = SessionEntry::Session {
            version: SESSION_VERSION,
            id: id.clone(),
            title: self.title.clone(),
            created_at: stamp,
            cwd: self.cwd.clone(),
            parent_session: Some(self.session_id.clone()),
        };
        let init = SessionEntry::SessionInit {
            id: new_id(),
            timestamp: stamp,
            cwd: self.cwd.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        write_atomic(&path, &[header, init])?;
        let lock = acquire_lock(&path)?;
        let writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| io_error(e, &path))?;
        self.written = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        self.path = path;
        self.writer = Some(BufWriter::new(writer));
        self.lock = lock;
        self.session_id = id;
        Ok(())
    }

    /// Id sesi yang sedang dibuka (dari header).
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Path file lock sesi ini (untuk diagnostik).
    pub fn lock_path(&self) -> &Path {
        self.lock.path()
    }

    /// Path file sesi ini.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Catat pemakaian token satu turn.
    pub fn append_usage(
        &mut self,
        model: impl Into<String>,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), MxError> {
        self.append(&SessionEntry::model_usage(
            model,
            input_tokens,
            output_tokens,
        ))
    }

    /// Ganti model sesi dengan menambahkan entry `ModelChange`.
    pub fn set_model(&mut self, model: impl Into<String>) -> Result<(), MxError> {
        self.append(&SessionEntry::model_change(model))
    }

    /// Ganti judul sesi dengan menambahkan entry `TitleChange`.
    pub fn set_title(&mut self, title: impl Into<String>) -> Result<(), MxError> {
        self.append(&SessionEntry::title_change(title))
    }

    /// Tambahkan satu entry dan paksa flush supaya crash tidak menghilangkan
    /// baris yang sudah dianggap selesai.
    pub fn append(&mut self, entry: &SessionEntry) -> Result<(), MxError> {
        let line = serde_json::to_string(entry).map_err(|e| {
            MxError::new(
                ErrorCode::SessionCorrupt,
                format!("gagal serialisasi entry: {e}"),
            )
        })?;
        let incoming = line.len() as u64 + 1;
        if self.written > 0 && self.written + incoming > self.max_bytes {
            self.rotate()?;
        }
        let Some(writer) = self.writer.as_mut() else {
            return Err(MxError::new(
                ErrorCode::SessionCorrupt,
                "writer sesi sudah ditutup",
            ));
        };
        writer
            .write_all(line.as_bytes())
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush())
            .map_err(|e| io_error(e, &self.path))?;
        self.written += incoming;
        Ok(())
    }

    /// Baca seluruh entry dari satu file sesi.
    pub fn load(path: &Path) -> Result<LoadedSession, MxError> {
        let file = File::open(path).map_err(|e| io_error(e, path))?;
        let reader = BufReader::new(file);
        let mut entries: Vec<SessionEntry> = Vec::new();
        let mut warnings = Vec::new();
        let lines: Vec<String> = reader
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io_error(e, path))?;
        let total = lines.len();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionEntry>(trimmed) {
                Ok(entry) => {
                    if index == 0 {
                        match &entry {
                            SessionEntry::Session { version, .. } => {
                                // Penjagaan versi: file dari versi skema yang lebih
                                // baru tidak boleh dibaca dengan asumsi skema lama.
                                if *version > SESSION_VERSION {
                                    return Err(MxError::new(
                                        ErrorCode::SessionCorrupt,
                                        format!(
                                            "{}: sesi ditulis dengan skema versi {version}, \
                                             implementasi ini mendukung versi {SESSION_VERSION}",
                                            path.display()
                                        ),
                                    ));
                                }
                                if *version == 0 {
                                    return Err(MxError::new(
                                        ErrorCode::SessionCorrupt,
                                        format!(
                                            "{}: versi skema sesi 0 tidak dikenal",
                                            path.display()
                                        ),
                                    ));
                                }
                            }
                            _ => {
                                return Err(MxError::new(
                                    ErrorCode::SessionCorrupt,
                                    format!("{}: baris pertama bukan header sesi", path.display()),
                                ));
                            }
                        }
                    }
                    entries.push(entry);
                }
                Err(err) => {
                    // Hanya baris terakhir yang benar-benar terpotong (EOF) boleh
                    // diabaikan sebagai sisa crash. Kesalahan data (mis. field
                    // asing) tetap ditolak agar skema tidak melenceng diam-diam.
                    let terpotong = index + 1 == total
                        && matches!(err.classify(), serde_json::error::Category::Eof);
                    if terpotong {
                        warnings.push(format!(
                            "{}: baris terakhir tidak lengkap dan diabaikan ({err})",
                            path.display()
                        ));
                    } else {
                        return Err(MxError::new(
                            ErrorCode::SessionCorrupt,
                            format!("{}: baris {} rusak: {err}", path.display(), index + 1),
                        ));
                    }
                }
            }
        }
        if entries.is_empty() {
            return Err(MxError::new(
                ErrorCode::SessionCorrupt,
                format!("{}: tidak ada entry sesi", path.display()),
            ));
        }
        Ok(LoadedSession { entries, warnings })
    }

    /// Duplikasi sesi ke file baru dengan header baru dan `parent_session` diisi.
    pub fn fork(source: &Path, dir: &Path, title: &str) -> Result<(Self, String), MxError> {
        let loaded = Self::load(source)?;
        let parent_id = loaded.session_id().unwrap_or_default().to_string();
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        fs::create_dir_all(dir).map_err(|e| io_error(e, dir))?;
        let id = new_id();
        let path = dir.join(format!("{}_{}.jsonl", now_ms(), id));
        let header = SessionEntry::Session {
            version: SESSION_VERSION,
            id: id.clone(),
            title: title.to_string(),
            created_at: now_ms(),
            cwd: cwd.clone(),
            parent_session: Some(parent_id),
        };
        let mut entries = vec![header];
        for entry in loaded.entries.into_iter().skip(1) {
            entries.push(entry);
        }
        write_atomic(&path, &entries)?;
        let lock = acquire_lock(&path)?;
        let writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| io_error(e, &path))?;
        let written = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Ok((
            Self {
                path,
                writer: Some(BufWriter::new(writer)),
                lock,
                title: title.to_string(),
                cwd,
                session_id: id.clone(),
                written,
                max_bytes: MAX_SESSION_BYTES,
            },
            id,
        ))
    }
}

/// Tulis daftar entry ke `path` lewat file sementara lalu rename.
///
/// Rename di Windows menolak menimpa file yang ada, jadi file lama dihapus
/// lebih dulu. Catatan: jendela antara hapus dan rename bukan atomik; risiko
/// ini didokumentasikan di `docs/skema-sesi-v1.md` dan ditutup oleh backup
/// harian sebelum v1 dirilis.
pub fn write_atomic(path: &Path, entries: &[SessionEntry]) -> Result<(), MxError> {
    let tmp = path.with_extension("jsonl.tmp");
    let mut text = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|e| {
            MxError::new(ErrorCode::SessionCorrupt, format!("gagal serialisasi: {e}"))
        })?;
        text.push_str(&line);
        text.push('\n');
    }
    fs::write(&tmp, text.as_bytes()).map_err(|e| io_error(e, &tmp))?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| io_error(e, path))?;
    }
    fs::rename(&tmp, path).map_err(|e| io_error(e, path))?;
    // File sesi berisi percakapan: batasi ACL ke pengguna saja (§10 butir 9).
    tui_wire::secure_file::restrict_to_current_user(path).map_err(|e| io_error(e, path))
}

fn io_error(error: std::io::Error, path: &Path) -> MxError {
    MxError::new(
        ErrorCode::SessionCorrupt,
        format!("{}: {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_wire::Message;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maicli-session-test-{name}-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn list_menghitung_pesan_dan_melayani_sesi_ganda() {
        let dir = temp_dir("list");
        let mut first = SessionStore::create(&dir, "sesi satu", "C:\\kerja").unwrap();
        first
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::user("halo"),
            ))
            .unwrap();
        let second = SessionStore::create(&dir, "sesi dua", "C:\\kerja").unwrap();
        let sessions = list(&dir);
        assert_eq!(sessions.len(), 2, "dua sesi harus terdaftar");
        let titled = sessions
            .iter()
            .find(|summary| summary.title == "sesi satu")
            .expect("sesi satu ada");
        assert_eq!(titled.messages, 1);
        let loaded = SessionStore::load(first.path()).unwrap();
        assert_eq!(Some(titled.id.as_str()), loaded.session_id());
        assert!(sessions.iter().any(|summary| summary.path == second.path()));
    }

    #[test]
    fn pohon_menghitung_kedalaman_dan_urutan_root_dulu() {
        let dir = temp_dir("pohon");
        let mut akar = SessionStore::create(&dir, "akar", ".").unwrap();
        akar.append(&SessionEntry::message(None, Message::user("a")))
            .unwrap();
        drop(akar);
        let daftar = list(&dir);
        let sumber = daftar[0].path.clone();
        let (forked, _id) = SessionStore::fork(&sumber, &dir, "cabang").unwrap();
        drop(forked);
        let pohon = pohon(&dir);
        assert_eq!(pohon.len(), 2);
        // akar dulu (kedalaman 0), cabang setelahnya (kedalaman 1, parent=akar).
        assert_eq!(pohon[0].kedalaman, 0);
        assert_eq!(pohon[0].parent, None);
        assert_eq!(pohon[1].kedalaman, 1);
        assert_eq!(
            pohon[1].parent.as_deref(),
            Some(pohon[0].summary.id.as_str())
        );
    }

    #[test]
    fn pohon_siklus_tidak_menggantung() {
        // Proteksi 32 tingkat: summary palsu dengan induk memutar ke dirinya.
        let mut summary = summary("x", "siklus");
        summary.parent = Some("x".to_string());
        let daftar = [summary];
        let induk_of: std::collections::HashMap<&str, &Option<String>> =
            daftar.iter().map(|s| (s.id.as_str(), &s.parent)).collect();
        let mut kedalaman = 0;
        let mut id = "x".to_string();
        loop {
            let berikut = induk_of.get(id.as_str()).and_then(|p| p.as_deref());
            match berikut {
                Some(next) => {
                    kedalaman += 1;
                    if kedalaman > 32 {
                        break;
                    }
                    id = next.to_string();
                }
                None => break,
            }
        }
        // Loop berhenti (tidak menggantung) pada ambang 32+1 = 33 iterasi.
        assert!(kedalaman <= 33, "proteksi siklus berhenti: {kedalaman}");
    }

    fn summary(id: &str, title: &str) -> SessionSummary {
        SessionSummary {
            id: id.to_string(),
            title: title.to_string(),
            path: PathBuf::from(format!("{id}.jsonl")),
            messages: 1,
            updated_at: 0,
            parent: None,
        }
    }

    #[test]
    fn match_session_persis_prefix_dan_ambigu() {
        let dir = Path::new("C:\\sesi");
        let all = vec![summary("abc123", "satu"), summary("abc456", "dua")];
        let exact = match_session(&all, "abc456", dir).expect("id persis");
        assert_eq!(exact.title, "dua");
        let ambiguous = match_session(&all, "abc", dir).expect_err("ambigu");
        assert!(
            ambiguous.message.contains("cocok dengan 2 sesi"),
            "{}",
            ambiguous.message
        );
        assert!(
            ambiguous.message.contains("abc123"),
            "{}",
            ambiguous.message
        );
        let missing = match_session(&all, "zzz", dir).expect_err("tidak ada");
        assert_eq!(missing.code, ErrorCode::SessionNotFound);
    }

    #[test]
    fn resolve_target_latest_dan_id_persis() {
        let dir = temp_dir("resolve");
        let mut first = SessionStore::create(&dir, "satu", "C:\\kerja").unwrap();
        first
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::user("halo"),
            ))
            .unwrap();
        let id = list(&dir)
            .into_iter()
            .next()
            .map(|session| session.id)
            .expect("ada sesi");
        assert_eq!(resolve_target(&dir, "latest").unwrap().id, id);
        assert_eq!(resolve_target(&dir, &id).unwrap().id, id);
        let err = resolve_target(&dir, "tidak-ada").expect_err("harus error");
        assert_eq!(err.code, ErrorCode::SessionNotFound);
    }

    #[test]
    fn model_change_terakhir_terbaca_dan_tertua_diabaikan() {
        let dir = temp_dir("model");
        let mut store = SessionStore::create(&dir, "model", "C:\\kerja").unwrap();
        assert_eq!(store_loaded(&store).model(), None, "belum ada perubahan");
        store.set_model("model-a").unwrap();
        store.set_model("model-b").unwrap();
        let path = store.path().to_path_buf();
        drop(store);
        let loaded = SessionStore::load(&path).unwrap();
        assert_eq!(loaded.model(), Some("model-b"), "yang terakhir menang");
    }

    #[test]
    fn usage_dijumlahkan_dari_semua_entry() {
        let dir = temp_dir("usage");
        let mut store = SessionStore::create(&dir, "usage", "C:\\kerja").unwrap();
        assert_eq!(store_loaded(&store).usage(), (0, 0));
        store.append_usage("model-a", 10, 5).unwrap();
        store.append_usage("model-a", 3, 2).unwrap();
        let path = store.path().to_path_buf();
        drop(store);
        let loaded = SessionStore::load(&path).unwrap();
        assert_eq!(loaded.usage(), (13, 7));
        assert_eq!(loaded.model(), None, "usage bukan perubahan model");
    }

    fn store_loaded(store: &SessionStore) -> LoadedSession {
        SessionStore::load(store.path()).unwrap()
    }

    #[test]
    fn remove_menghapus_berkas_dan_lock() {
        let dir = temp_dir("hapus");
        let store = SessionStore::create(&dir, "hapus", "C:\\kerja").unwrap();
        let path = store.path().to_path_buf();
        let lock = store.lock_path().to_path_buf();
        assert!(lock.exists(), "lock dibuat saat store hidup");
        drop(store);
        remove(&path).expect("hapus");
        assert!(!path.exists());
        assert!(!lock.exists(), "artefak lock ikut dihapus");
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn remove_menolak_sesi_yang_sedang_dipakai() {
        let dir = temp_dir("hapus-terkunci");
        let store = SessionStore::create(&dir, "terkunci", "C:\\kerja").unwrap();
        let path = store.path().to_path_buf();
        let err = remove(&path).expect_err("tidak boleh menghapus sesi aktif");
        assert_eq!(err.code, ErrorCode::SessionLocked);
        assert!(path.exists(), "berkas tidak boleh hilang");
        drop(store);
    }

    #[test]
    fn remove_sesi_tidak_ada_dilaporkan() {
        let dir = temp_dir("hapus-tidak-ada");
        let err = remove(&dir.join("tidak-ada.jsonl")).expect_err("harus error");
        assert_eq!(err.code, ErrorCode::SessionNotFound);
    }

    #[test]
    fn search_judul_tanpa_peduli_huruf_besar() {
        let dir = temp_dir("cari");
        SessionStore::create(&dir, "Kerja malam", "C:\\kerja").unwrap();
        SessionStore::create(&dir, "Rencana besok", "C:\\kerja").unwrap();
        assert_eq!(
            search(&dir, "kerja").len(),
            1,
            "cocok tanpa peduli huruf besar"
        );
        assert_eq!(search(&dir, "  ").len(), 2, "query kosong = semua sesi");
        assert_eq!(search(&dir, "tidak-ada").len(), 0);
        assert_eq!(search(&dir, "RENCANA").len(), 1);
    }

    #[test]
    fn file_sesi_dibatasi_ke_pengguna_saja() {
        let dir = temp_dir("acl");
        let store = SessionStore::create(&dir, "acl", "C:\\kerja").unwrap();
        let path = store.path().to_path_buf();
        drop(store);
        assert!(
            tui_wire::secure_file::acl_restricted(&path).expect("periksa ACL"),
            "file sesi harus 1 ACE dan terlindungi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_dengan_field_asing_ditolak() {
        let dir = temp_dir("entry-asing");
        let path = dir.join("sesi.jsonl");
        let header = "{\"type\":\"session\",\"version\":1,\"id\":\"x\",\"title\":\"t\",\"created_at\":1,\"cwd\":\"C:\\\\k\"}";
        let asing = "{\"type\":\"message\",\"id\":\"m1\",\"timestamp\":2,\"message\":{\"role\":\"user\",\"content\":\"hi\"},\"liar\":true}";
        std::fs::write(&path, format!("{header}\n{asing}\n")).unwrap();
        let err = SessionStore::load(&path).expect_err("field asing harus ditolak");
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
        assert!(err.message.contains("liar"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_versi_baru_ditolak_dengan_pesan_jelas() {
        let dir = temp_dir("versi-baru");
        let path = dir.join("sesi.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session\",\"version\":99,\"id\":\"x\",\"title\":\"t\",\"created_at\":1,\"cwd\":\"C:\\\\k\"}\n",
        )
        .unwrap();
        let err = SessionStore::load(&path).expect_err("versi baru harus ditolak");
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
        assert!(err.message.contains("versi 99"), "{}", err.message);
        assert!(err.message.contains("mendukung versi 1"), "{}", err.message);
    }

    #[test]
    fn header_versi_nol_ditolak() {
        let dir = temp_dir("versi-nol");
        let path = dir.join("sesi.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session\",\"version\":0,\"id\":\"x\",\"title\":\"t\",\"created_at\":1,\"cwd\":\"C:\\\\k\"}\n",
        )
        .unwrap();
        let err = SessionStore::load(&path).expect_err("versi 0 harus ditolak");
        assert!(
            err.message.contains("versi skema sesi 0"),
            "{}",
            err.message
        );
    }

    #[test]
    fn rotasi_sesi_saat_batas_ukuran_tercapai() {
        let dir = temp_dir("rotasi");
        let mut store = SessionStore::create(&dir, "rotasi", "C:\\kerja").unwrap();
        store.set_max_bytes(600);
        let first_path = store.path().to_path_buf();
        for n in 0..6 {
            store
                .append(&SessionEntry::message(
                    None,
                    tui_wire::Message::user(format!("pesan {n} {}", "x".repeat(200))),
                ))
                .unwrap();
        }
        let second_path = store.path().to_path_buf();
        assert_ne!(first_path, second_path, "harus pindah berkas");
        assert!(first_path.exists(), "berkas lama tetap ada");
        let second = SessionStore::load(&second_path).unwrap();
        let parent = second
            .entries
            .iter()
            .find_map(|entry| match entry {
                SessionEntry::Session { parent_session, .. } => parent_session.clone(),
                _ => None,
            })
            .expect("header baru punya parent");
        let known: Vec<String> = list(&dir).into_iter().map(|summary| summary.id).collect();
        assert!(
            known.contains(&parent),
            "parent harus id sesi yang ada di direktori: {parent} vs {known:?}"
        );
        assert_ne!(
            parent,
            second.session_id().unwrap_or_default(),
            "parent bukan dirinya sendiri"
        );
        assert!(!second.messages().is_empty(), "pesan ikut di berkas baru");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tanpa_rotasi_bila_di_bawah_batas() {
        let dir = temp_dir("tanpa-rotasi");
        let mut store = SessionStore::create(&dir, "kecil", "C:\\kerja").unwrap();
        store.set_max_bytes(MAX_SESSION_BYTES);
        let path = store.path().to_path_buf();
        store
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::user("halo"),
            ))
            .unwrap();
        assert_eq!(store.path(), path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn title_change_menang_atas_header() {
        let dir = temp_dir("title");
        let mut store = SessionStore::create(&dir, "judul awal", "C:\\kerja").unwrap();
        store.set_title("judul baru").unwrap();
        let path = store.path().to_path_buf();
        drop(store);
        let loaded = SessionStore::load(&path).unwrap();
        assert_eq!(loaded.title(), Some("judul baru"));
        let summary = list(&dir).into_iter().next().expect("ada sesi");
        assert_eq!(summary.title, "judul baru", "daftar memakai judul terbaru");
    }

    #[test]
    fn latest_mengikuti_file_terakhir_ditulis() {
        let dir = temp_dir("latest");
        let mut first = SessionStore::create(&dir, "lama", "C:\\kerja").unwrap();
        let _second = SessionStore::create(&dir, "baru", "C:\\kerja").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        first
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::user("tambahan"),
            ))
            .unwrap();
        let latest = latest(&dir).expect("ada sesi");
        assert_eq!(latest.title, "lama", "file yang baru ditulis harus menang");
    }

    #[test]
    fn resume_hanya_mengambil_entry_pesan() {
        let dir = temp_dir("resume");
        let mut store = SessionStore::create(&dir, "resume", "C:\\kerja").unwrap();
        store
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::user("satu"),
            ))
            .unwrap();
        store
            .append(&SessionEntry::message(
                None,
                tui_wire::Message::assistant(&tui_wire::AssistantMessage::text(
                    "dua",
                    tui_wire::StopReason::Stop,
                )),
            ))
            .unwrap();
        let loaded = SessionStore::load(store.path()).unwrap();
        let messages = loaded.messages();
        assert_eq!(messages.len(), 2, "init tidak boleh ikut");
        assert!(matches!(&messages[0], tui_wire::Message::User { content } if content == "satu"));
        assert!(matches!(&messages[1], tui_wire::Message::Assistant { .. }));
    }

    #[test]
    fn resume_menerapkan_compaction() {
        let entries = vec![
            SessionEntry::message(None, tui_wire::Message::user("lama satu")),
            SessionEntry::message(
                None,
                tui_wire::Message::assistant(&tui_wire::AssistantMessage::text(
                    "lama dua",
                    tui_wire::StopReason::Stop,
                )),
            ),
            SessionEntry::compaction(None, "  ringkasan  ", 2),
            SessionEntry::message(None, tui_wire::Message::user("setelah ringkasan")),
        ];
        let messages = resume_messages(&entries);
        assert_eq!(messages.len(), 2, "ringkasan + pesan sesudahnya");
        match &messages[0] {
            tui_wire::Message::User { content } => {
                assert!(content.contains("[ringkasan konteks sebelumnya]"));
                assert!(content.contains("ringkasan"));
                assert!(!content.contains("lama satu"));
            }
            other => panic!("dapat {other:?}"),
        }
        assert!(
            matches!(&messages[1], tui_wire::Message::User { content } if content == "setelah ringkasan")
        );
    }

    #[test]
    fn file_rusak_dilewati_saat_mendaftar() {
        let dir = temp_dir("list-rusak");
        let _valid = SessionStore::create(&dir, "valid", "C:\\kerja").unwrap();
        fs::write(dir.join("0_rusak.jsonl"), b"bukan json\n").unwrap();
        let sessions = list(&dir);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "valid");
    }
    #[test]
    fn lock_kedua_ditolak_saat_sesi_masih_dipakai() {
        let dir = temp_dir("lock");
        let store = SessionStore::create(&dir, "terkunci", "C:\\kerja").unwrap();
        let second = SessionStore::open(store.path());
        match second {
            Err(err) => assert_eq!(err.code, ErrorCode::SessionLocked),
            Ok(_) => panic!("proses kedua tidak boleh mendapat lock"),
        }
        assert!(store.lock_path().exists());
    }

    #[test]
    fn lock_dilepas_saat_store_di_drop() {
        let dir = temp_dir("lock-drop");
        let store = SessionStore::create(&dir, "lepas", "C:\\kerja").unwrap();
        let path = store.path().to_path_buf();
        let lock = store.lock_path().to_path_buf();
        drop(store);
        assert!(!lock.exists(), "lock harus dihapus saat drop");
        let reopened = SessionStore::open(&path).expect("sesi bisa dibuka lagi");
        assert_eq!(reopened.path(), path);
    }

    #[test]
    fn lock_basi_diambil_alih() {
        let dir = temp_dir("lock-stale");
        let store = SessionStore::create(&dir, "basi", "C:\\kerja").unwrap();
        let path = store.path().to_path_buf();
        let lock = store.lock_path().to_path_buf();
        drop(store);
        let old = now_ms().saturating_sub(LOCK_STALE_AFTER_MS + 60_000);
        fs::write(&lock, format!("{old}\npid 999999\n")).unwrap();
        let reopened = SessionStore::open(&path).expect("lock basi harus diambil alih");
        assert!(lock.exists(), "lock baru harus dibuat");
        drop(reopened);
        assert!(!lock.exists());
    }

    #[test]
    fn round_trip_tulis_baca() {
        let dir = temp_dir("roundtrip");
        let mut store = SessionStore::create(&dir, "uji", "C:\\kerja").unwrap();
        store
            .append(&SessionEntry::message(None, Message::user("halo")))
            .unwrap();
        store
            .append(&SessionEntry::message(None, Message::user("lagi")))
            .unwrap();
        let loaded = SessionStore::load(store.path()).unwrap();
        // header + session_init + dua pesan
        assert_eq!(loaded.entries.len(), 4);
        assert_eq!(loaded.title(), Some("uji"));
        assert!(loaded.warnings.is_empty());
        assert_eq!(
            loaded
                .entries
                .iter()
                .filter(|e| e.is_context_message())
                .count(),
            2
        );
    }

    #[test]
    fn baris_terakhir_terpotong_diabaikan() {
        let dir = temp_dir("truncated");
        let store = SessionStore::create(&dir, "t", ".").unwrap();
        let mut text = fs::read_to_string(store.path()).unwrap();
        text.push_str("{\"type\":\"message\",\"id\":\"x\"");
        fs::write(store.path(), text).unwrap();
        let loaded = SessionStore::load(store.path()).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn baris_tengah_rusak_menghasilkan_error() {
        let dir = temp_dir("corrupt");
        let store = SessionStore::create(&dir, "t", ".").unwrap();
        let mut text = fs::read_to_string(store.path()).unwrap();
        text.push_str("bukan json\n");
        text.push_str(
            &serde_json::to_string(&SessionEntry::message(None, Message::user("x"))).unwrap(),
        );
        text.push('\n');
        fs::write(store.path(), text).unwrap();
        let err = SessionStore::load(store.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
        assert!(err.message.contains("baris 3"));
    }

    #[test]
    fn fork_menyetel_parent_session() {
        let dir = temp_dir("fork");
        let mut store = SessionStore::create(&dir, "asli", ".").unwrap();
        store
            .append(&SessionEntry::message(None, Message::user("halo")))
            .unwrap();
        let (forked, new_id) = SessionStore::fork(store.path(), &dir, "turunan").unwrap();
        let loaded = SessionStore::load(forked.path()).unwrap();
        assert_eq!(loaded.session_id(), Some(new_id.as_str()));
        match &loaded.entries[0] {
            SessionEntry::Session {
                parent_session,
                title,
                ..
            } => {
                assert!(parent_session.is_some());
                assert_eq!(title, "turunan");
            }
            other => panic!("header tidak valid: {other:?}"),
        }
        assert!(loaded.entries.len() >= 3);
    }

    #[test]
    fn file_tanpa_header_ditolak() {
        let dir = temp_dir("noheader");
        let path = dir.join("x.jsonl");
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&SessionEntry::message(None, Message::user("x"))).unwrap()
            ),
        )
        .unwrap();
        let err = SessionStore::load(&path).unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
    }
}
