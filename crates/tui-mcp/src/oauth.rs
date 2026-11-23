//! OAuth 2.1 authorization-code + PKCE untuk server MCP.
//!
//! Alur: discovery metadata (bila endpoint kosong) → loopback listener →
//! browser → callback `code` + `state` → tukar token → simpan 0600. Token bisa
//! di-refresh tanpa membuka browser lagi.

use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tui_wire::{ErrorCode, MxError};

/// Konfigurasi OAuth satu server MCP.
#[derive(Debug, Clone, Default)]
pub struct OAuthConfig {
    /// Client id.
    pub client_id: String,
    /// Endpoint authorization (kosong = discovery).
    pub authorization_url: String,
    /// Endpoint token (kosong = discovery).
    pub token_url: String,
    /// Scope.
    pub scopes: Vec<String>,
    /// Port loopback; 0 = acak.
    pub redirect_port: u16,
    /// URL server MCP untuk discovery.
    pub server_url: String,
}

/// Token OAuth yang disimpan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Token {
    /// Access token.
    pub access_token: String,
    /// Refresh token (bila ada).
    #[serde(default)]
    pub refresh_token: String,
    /// Waktu kedaluwarsa (ms epoch; 0 = tidak diketahui).
    #[serde(default)]
    pub expires_at_ms: u64,
    /// Tipe token.
    #[serde(default)]
    pub token_type: String,
}

impl Token {
    /// Apakah token kedaluwarsa (dengan margin 60 detik).
    pub fn expired(&self) -> bool {
        self.expires_at_ms != 0 && now_ms().saturating_add(60_000) >= self.expires_at_ms
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    token_type: String,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::Config, pesan.into())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn acak(bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes);
    while out.len() < bytes {
        let state = std::collections::hash_map::RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(now_ms());
        hasher.write_u32(std::process::id());
        hasher.write_usize(out.len());
        out.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    out.truncate(bytes);
    out
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Verifier PKCE acak (43–128 karakter base64url).
pub fn code_verifier() -> String {
    b64url(&acak(32))
}

/// Challenge S256 dari verifier.
pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    b64url(&digest)
}

fn persen_encode(nilai: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(nilai.len());
    for byte in nilai.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[(byte >> 4) as usize]));
            out.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    out
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn persen_decode(nilai: &str) -> String {
    let bytes = nilai.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_val(bytes[index + 1]), hex_val(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parse query string `a=1&b=2` menjadi pasangan (decoded).
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (persen_decode(key), persen_decode(value))
        })
        .collect()
}

/// URL authorization dengan parameter PKCE.
pub fn authorization_url(
    cfg: &OAuthConfig,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<String, MxError> {
    if cfg.authorization_url.trim().is_empty() {
        return Err(galat("authorization_url kosong (discovery gagal?)"));
    }
    let pemisah = if cfg.authorization_url.contains('?') {
        '&'
    } else {
        '?'
    };
    let mut url = format!(
        "{}{}response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        cfg.authorization_url,
        pemisah,
        persen_encode(&cfg.client_id),
        persen_encode(redirect_uri),
        persen_encode(state),
        persen_encode(challenge)
    );
    if !cfg.scopes.is_empty() {
        url.push_str(&format!("&scope={}", persen_encode(&cfg.scopes.join(" "))));
    }
    Ok(url)
}

fn buka_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(url);
        c
    };
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Simpan token dengan izin pengguna saja.
pub fn save_token(path: &Path, token: &Token) -> Result<(), MxError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| galat(format!("{}: {e}", parent.display())))?;
    }
    let text = serde_json::to_string_pretty(token).map_err(|e| galat(e.to_string()))?;
    std::fs::write(path, text).map_err(|e| galat(format!("{}: {e}", path.display())))?;
    tui_wire::secure_file::restrict_to_current_user(path)
        .map_err(|e| galat(format!("{}: {e}", path.display())))?;
    Ok(())
}

/// Baca token dari disk bila ada.
pub fn load_token(path: &Path) -> Option<Token> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Path token satu server.
pub fn token_path(config_root: &Path, server: &str) -> PathBuf {
    config_root.join("mcp-oauth").join(format!("{server}.json"))
}

async fn discovery(server_url: &str) -> Result<Metadata, MxError> {
    let Some((scheme, rest)) = server_url.split_once("://") else {
        return Err(galat(format!("URL server MCP tidak valid: {server_url}")));
    };
    let origin = rest.split('/').next().unwrap_or(rest);
    let url = format!("{scheme}://{origin}/.well-known/oauth-authorization-server");
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| galat(format!("discovery OAuth gagal: {e}")))?;
    if !response.status().is_success() {
        return Err(galat(format!(
            "discovery OAuth HTTP {} dari {url}",
            response.status()
        )));
    }
    response
        .json::<Metadata>()
        .await
        .map_err(|e| galat(format!("metadata OAuth tidak valid: {e}")))
}

async fn tukar_code(
    token_url: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<Token, MxError> {
    let body = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    let response = reqwest::Client::new()
        .post(token_url)
        .form(&body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| galat(format!("tukar token gagal: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| galat(format!("respons token tidak terbaca: {e}")))?;
    if !status.is_success() {
        return Err(galat(format!("token endpoint HTTP {status}: {text}")));
    }
    let parsed: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| galat(format!("respons token tidak valid: {e}")))?;
    Ok(Token {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_at_ms: now_ms().saturating_add(parsed.expires_in.saturating_mul(1_000)),
        token_type: parsed.token_type,
    })
}

async fn refresh(token: &Token, cfg: &OAuthConfig) -> Result<Token, MxError> {
    if token.refresh_token.is_empty() || cfg.token_url.is_empty() {
        return Err(galat("token kedaluwarsa dan tidak ada refresh token"));
    }
    let body = [
        ("grant_type", "refresh_token"),
        ("refresh_token", token.refresh_token.as_str()),
        ("client_id", cfg.client_id.as_str()),
    ];
    let response = reqwest::Client::new()
        .post(&cfg.token_url)
        .form(&body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| galat(format!("refresh token gagal: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| galat(format!("respons refresh tidak terbaca: {e}")))?;
    if !status.is_success() {
        return Err(galat(format!("refresh token HTTP {status}: {text}")));
    }
    let parsed: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| galat(format!("respons refresh tidak valid: {e}")))?;
    Ok(Token {
        access_token: parsed.access_token,
        refresh_token: if parsed.refresh_token.is_empty() {
            token.refresh_token.clone()
        } else {
            parsed.refresh_token
        },
        expires_at_ms: now_ms().saturating_add(parsed.expires_in.saturating_mul(1_000)),
        token_type: parsed.token_type,
    })
}

/// Ambil access token valid; refresh bila kedaluwarsa.
pub async fn ensure_access_token(path: &Path, cfg: &OAuthConfig) -> Result<String, MxError> {
    let token =
        load_token(path).ok_or_else(|| galat("belum ada token OAuth; jalankan /mcp reauth"))?;
    if !token.expired() {
        return Ok(token.access_token);
    }
    let baru = refresh(&token, cfg).await?;
    save_token(path, &baru)?;
    Ok(baru.access_token)
}

/// Jalankan alur OAuth lengkap dan simpan token.
pub async fn reauth(cfg: &OAuthConfig, path: &Path) -> Result<Token, MxError> {
    let mut cfg = cfg.clone();
    if cfg.authorization_url.is_empty() || cfg.token_url.is_empty() {
        if cfg.server_url.is_empty() {
            return Err(galat("endpoint OAuth kosong dan server_url tidak diisi"));
        }
        let meta = discovery(&cfg.server_url).await?;
        cfg.authorization_url = meta.authorization_endpoint;
        cfg.token_url = meta.token_endpoint;
    }
    let listener = TcpListener::bind(("127.0.0.1", cfg.redirect_port))
        .await
        .map_err(|e| galat(format!("loopback OAuth gagal bind: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| galat(format!("alamat loopback tidak terbaca: {e}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let verifier = code_verifier();
    let challenge = code_challenge(&verifier);
    let state = b64url(&acak(16));
    let url = authorization_url(&cfg, &redirect_uri, &state, &challenge)?;
    buka_browser(&url).map_err(|e| {
        galat(format!(
            "browser tidak bisa dibuka ({e}); buka manual:\n{url}"
        ))
    })?;
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| galat("menunggu callback OAuth melewati 5 menit"))?
        .map_err(|e| galat(format!("callback OAuth gagal: {e}")))?;
    let mut buffer = vec![0u8; 8_192];
    let n = socket
        .read(&mut buffer)
        .await
        .map_err(|e| galat(format!("callback OAuth tidak terbaca: {e}")))?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| galat("callback OAuth tanpa request line"))?;
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params = parse_query(query);
    let kode = params
        .iter()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.clone())
        .ok_or_else(|| galat("callback OAuth tanpa code"))?;
    let state_diterima = params
        .iter()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    if state_diterima != state {
        return Err(galat("state OAuth tidak cocok (kemungkinan CSRF)"));
    }
    let _ = socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<h1>OAuth selesai</h1><p>Kembali ke maicli.</p>",
        )
        .await;
    let token = tukar_code(
        &cfg.token_url,
        &cfg.client_id,
        &kode,
        &redirect_uri,
        &verifier,
    )
    .await?;
    save_token(path, &token)?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn pkce_cocok_vektor_rfc7636() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_acak_dan_panjang_sah() {
        let a = code_verifier();
        let b = code_verifier();
        assert_ne!(a, b);
        assert!((43..=128).contains(&a.len()), "{}", a.len());
    }

    #[test]
    fn authorization_url_memuat_parameter() {
        let cfg = OAuthConfig {
            client_id: "client".to_string(),
            authorization_url: "https://auth.example/authorize".to_string(),
            scopes: vec!["mcp read".to_string()],
            ..OAuthConfig::default()
        };
        let url = authorization_url(&cfg, "http://127.0.0.1:9/cb", "s", "c").unwrap();
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("scope=mcp%20read"), "{url}");
    }

    #[test]
    fn query_decode_persen_dan_simbol() {
        let params = parse_query("code=a%2Bb&state=x%20y");
        assert_eq!(params[0], ("code".to_string(), "a+b".to_string()));
        assert_eq!(params[1], ("state".to_string(), "x y".to_string()));
    }

    #[test]
    fn query_decode_tidak_panic_untuk_persen_rusak() {
        let params = parse_query("q=%é&x=%2&y=%GG");
        assert_eq!(params[0].1, "%é");
        assert_eq!(params[1].1, "%2");
        assert_eq!(params[2].1, "%GG");
    }

    #[test]
    fn token_tersimpan_0600() {
        let dir = std::env::temp_dir().join(format!("maicli-oauth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("server.json");
        let token = Token {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_at_ms: now_ms() + 60_000,
            token_type: "Bearer".to_string(),
        };
        save_token(&path, &token).unwrap();
        assert_eq!(load_token(&path).unwrap(), token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
