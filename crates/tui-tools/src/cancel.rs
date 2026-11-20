//! Pembatalan kooperatif untuk eksekusi tool.
//!
//! Tool yang berjalan lama (khususnya `bash`) harus bisa dihentikan dari UI
//! tanpa menunggu timeout. Token ini dishare lewat `ToolContext`; `cancel()`
//! memicu `cancelled()` pada semua pemegang salinan.

use std::sync::Arc;

use tokio::sync::watch;

/// Token pembatalan; salinan berbagi keadaan yang sama.
#[derive(Debug, Clone)]
pub struct CancelToken {
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    /// Token baru dalam keadaan belum dibatalkan.
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    /// Tandai dibatalkan; idempoten.
    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    /// Kembalikan ke keadaan awal untuk turn berikutnya.
    pub fn reset(&self) {
        let _ = self.tx.send(false);
    }

    /// Apakah pembatalan sudah diminta.
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Menunggu sampai pembatalan diminta.
    ///
    /// Bila sudah dibatalkan sebelum dipanggil, langsung kembali.
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_dan_reset_mengubah_keadaan() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled(), "salinan harus melihat pembatalan");
        token.reset();
        assert!(!clone.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_menunggu_pembatalan() {
        let token = CancelToken::new();
        let clone = token.clone();
        let waiter = tokio::spawn(async move { clone.cancelled().await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        token.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn cancelled_langsung_kembali_bila_sudah_dibatalkan() {
        let token = CancelToken::new();
        token.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), token.cancelled())
                .await
                .is_ok()
        );
    }
}
