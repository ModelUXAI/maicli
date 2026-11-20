//! Error gateway dengan pemetaan ke taksonomi stabil.

use std::time::Duration;

use tui_wire::ErrorCode;

/// Kegagalan saat berbicara dengan gateway.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct GatewayError {
    /// Kode stabil untuk UI dan sesi.
    pub code: ErrorCode,
    /// Status HTTP bila ada.
    pub status: Option<u16>,
    /// Pesan teknis singkat.
    pub message: String,
    /// Apakah percobaan ulang boleh dilakukan.
    pub retryable: bool,
    /// Nilai `retry-after` dari server, bila ada.
    pub retry_after: Option<Duration>,
}

impl GatewayError {
    /// Error transport (tidak terjangkau).
    pub fn unreachable(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::GatewayUnreachable,
            status: None,
            message: message.into(),
            retryable: true,
            retry_after: None,
        }
    }

    /// Petakan status HTTP ke kode stabil.
    pub fn from_status(status: u16, body: &str) -> Self {
        let message = body.chars().take(400).collect::<String>();
        match status {
            401 | 403 => Self {
                code: ErrorCode::GatewayAuth,
                status: Some(status),
                message,
                retryable: false,
                retry_after: None,
            },
            429 => Self {
                code: ErrorCode::GatewayRateLimit,
                status: Some(status),
                message,
                retryable: true,
                retry_after: None,
            },
            408 | 425 | 500..=599 => Self {
                code: ErrorCode::GatewayServer,
                status: Some(status),
                message,
                retryable: true,
                retry_after: None,
            },
            _ => Self {
                code: ErrorCode::GatewayRejected,
                status: Some(status),
                message,
                retryable: false,
                retry_after: None,
            },
        }
    }

    /// Error karena respons tidak bisa diparse.
    pub fn malformed(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::GatewayRejected,
            status: None,
            message: message.into(),
            retryable: false,
            retry_after: None,
        }
    }

    /// Tambahkan `retry-after` yang dipatuhi retry.
    pub fn with_retry_after(mut self, retry_after: Option<Duration>) -> Self {
        self.retry_after = retry_after;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_dipetakan_ke_kode_stabil() {
        assert_eq!(
            GatewayError::from_status(401, "").code,
            ErrorCode::GatewayAuth
        );
        assert_eq!(
            GatewayError::from_status(429, "").code,
            ErrorCode::GatewayRateLimit
        );
        assert_eq!(
            GatewayError::from_status(503, "").code,
            ErrorCode::GatewayServer
        );
        assert_eq!(
            GatewayError::from_status(400, "").code,
            ErrorCode::GatewayRejected
        );
    }

    #[test]
    fn hanya_kegagalan_sementara_yang_retryable() {
        assert!(GatewayError::unreachable("dns").retryable);
        assert!(GatewayError::from_status(429, "").retryable);
        assert!(GatewayError::from_status(502, "").retryable);
        assert!(!GatewayError::from_status(401, "").retryable);
        assert!(!GatewayError::from_status(400, "").retryable);
    }

    #[test]
    fn body_dipotong_di_pesan() {
        let body = "x".repeat(1000);
        let err = GatewayError::from_status(400, &body);
        assert_eq!(err.message.chars().count(), 400);
    }
}
