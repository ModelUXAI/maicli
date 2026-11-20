//! Membuka dan menulis store sesi di thread blocking.
//!
//! Aturan async proyek (`AGENTS.md` §4) mewajibkan fs sinkron lewat
//! `spawn_blocking`; membuka store membaca file, mengambil lock, dan menulis
//! header, jadi semuanya blocking.

use std::path::PathBuf;

use tui_session::SessionStore;
use tui_wire::{ErrorCode, MxError};

/// Tulis satu entry sesi di thread blocking.
///
/// Store diambil sementara (`take`) karena closure blocking menuntut
/// kepemilikan; setelah selesai store dikembalikan ke tempatnya. Bila task
/// blocking gagal (panic), store dilepas dan pemanggil berjalan tanpa
/// persistensi — lebih baik daripada memblokir worker runtime.
pub async fn append(store: &mut Option<SessionStore>, entry: tui_session::SessionEntry) {
    let Some(mut current) = store.take() else {
        return;
    };
    match tokio::task::spawn_blocking(move || {
        let _ = current.append(&entry);
        current
    })
    .await
    {
        Ok(returned) => *store = Some(returned),
        Err(_) => *store = None,
    }
}

/// Buat store sesi baru di `sessions_dir` (dipakai `/new`).
pub async fn create_new(sessions_dir: PathBuf, cwd: String) -> Result<SessionStore, MxError> {
    let created =
        tokio::task::spawn_blocking(move || SessionStore::create(&sessions_dir, "sesi mx", &cwd))
            .await;
    match created {
        Ok(result) => result,
        Err(_) => Err(MxError::new(
            ErrorCode::SessionCorrupt,
            "membuat sesi baru dibatalkan runtime".to_string(),
        )),
    }
}

/// Duplikasi sesi `source` (dipakai `/fork`) beserta pesan yang dimuat.
pub async fn fork_current(
    source: PathBuf,
    sessions_dir: PathBuf,
    title: String,
) -> Result<(SessionStore, Vec<tui_wire::Message>), MxError> {
    let forked = tokio::task::spawn_blocking(move || {
        let (store, _id) = SessionStore::fork(&source, &sessions_dir, &title)?;
        let loaded = SessionStore::load(store.path())?;
        Ok::<_, MxError>((store, loaded.messages()))
    })
    .await;
    match forked {
        Ok(result) => result,
        Err(_) => Err(MxError::new(
            ErrorCode::SessionCorrupt,
            "menduplikasi sesi dibatalkan runtime".to_string(),
        )),
    }
}

/// Buka sesi yang dilanjutkan, atau buat sesi baru di `sessions_dir`.
///
/// Galat dikembalikan (bukan diubah menjadi `None`) supaya pemanggil bisa
/// memberi tahu pengguna bahwa percakapan ini tidak akan tersimpan — sebelumnya
/// kegagalan seperti direktori tak bisa dibuat hilang tanpa jejak.
pub async fn open_or_create(
    sessions_dir: PathBuf,
    resume: Option<PathBuf>,
    cwd: String,
) -> Result<SessionStore, MxError> {
    let opened = tokio::task::spawn_blocking(move || match resume {
        Some(path) => SessionStore::open(&path),
        None => SessionStore::create(&sessions_dir, "sesi mx", &cwd),
    })
    .await;
    match opened {
        Ok(result) => result,
        Err(_) => Err(MxError::new(
            ErrorCode::SessionCorrupt,
            "membuka sesi dibatalkan runtime".to_string(),
        )),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mx-open-{name}-{}", tui_session::new_id()))
    }

    #[tokio::test]
    async fn create_new_membuat_sesi_baru() {
        let dir = temp_path("baru-command");
        let store = create_new(dir.clone(), "C:\\kerja".to_string())
            .await
            .expect("sesi dibuat");
        assert!(store.path().exists());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fork_current_menyalin_pesan() {
        let dir = temp_path("fork-open");
        let mut store = SessionStore::create(&dir, "asli", "C:\\kerja").expect("buat sesi");
        store
            .append(&tui_session::SessionEntry::message(
                None,
                tui_wire::Message::user("halo"),
            ))
            .expect("tulis pesan");
        let source = store.path().to_path_buf();
        drop(store);
        let (forked, messages) = fork_current(source, dir.clone(), "salinan".to_string())
            .await
            .expect("fork");
        assert_eq!(messages.len(), 1);
        assert!(forked.path().exists());
        drop(forked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn direktori_tak_bisa_dibuat_dilaporkan() {
        // `sessions_dir` berupa file, jadi `create_dir_all` gagal.
        let blocker = temp_path("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let err = match open_or_create(blocker.clone(), None, "C:\\kerja".to_string()).await {
            Err(err) => err,
            Ok(_) => panic!("harus gagal"),
        };
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
        let _ = std::fs::remove_file(&blocker);
    }

    #[tokio::test]
    async fn sesi_baru_dibuat_di_direktori_bersih() {
        let dir = temp_path("baru");
        let store = open_or_create(dir.clone(), None, "C:\\kerja".to_string())
            .await
            .expect("store dibuat");
        assert!(store.path().exists());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resume_ke_file_tidak_ada_dilaporkan() {
        let missing = temp_path("hilang").join("sesi.jsonl");
        let err =
            match open_or_create(temp_path("dir"), Some(missing), "C:\\kerja".to_string()).await {
                Err(err) => err,
                Ok(_) => panic!("harus gagal"),
            };
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
    }
}
