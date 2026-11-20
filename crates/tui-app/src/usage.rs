//! Pencatatan pemakaian token per turn ke file sesi.

use tui_core::AgentSession;
use tui_session::SessionStore;
use tui_wire::Usage;

/// Tulis selisih pemakaian sejak `persisted` sebagai satu entry `ModelUsage`.
///
/// Selisih (bukan total) supaya resume tidak menghitung token yang sama dua
/// kali; `persisted` ikut dimajukan ke total terbaru walau penulisan gagal,
/// karena kegagalan disk tidak boleh membuat entry ganda di turn berikutnya.
pub async fn persist_usage(
    store: &mut Option<SessionStore>,
    session: &AgentSession,
    persisted: &mut Usage,
) {
    let total = session.usage();
    let delta = total.delta_since(*persisted);
    if delta.is_zero() {
        return;
    }
    crate::session_open::append(
        store,
        tui_session::SessionEntry::model_usage(
            session.model(),
            delta.input_tokens,
            delta.output_tokens,
        ),
    )
    .await;
    *persisted = total;
}
