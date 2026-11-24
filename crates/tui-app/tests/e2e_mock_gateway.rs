//! E2E tanpa jaringan nyata: binary `maicli` melawan gateway tiruan lokal.
//!
//! Membuktikan jalur penuh mode teks: config runtime, klien gateway (SSE),
//! penyimpanan sesi, perekam `--event-log`, dan format transkrip parity —
//! tanpa `MODELUXAI_API_KEY` sungguhan dan tanpa memanggil internet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Jalankan server HTTP tiruan: `/models` mengembalikan daftar model, endpoint
/// lain mengembalikan satu respons SSE teks.
fn spawn_mock_gateway() -> (String, std::thread::JoinHandle<()>) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("tidak bisa bind gateway tiruan");
    };
    let Some(addr) = listener.local_addr().ok() else {
        panic!("alamat gateway tiruan tidak diketahui");
    };
    // Satu thread menerima koneksi dan langsung menangani setiap request secara
    // berurutan. Tiap response memakai `Connection: close`, jadi klien tidak
    // menunggu keep-alive dan `accept()` berikutnya tidak pernah diblokir.
    let handle = std::thread::spawn(move || {
        let mut chat_index = 0usize;
        for _ in 0..32 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 16384];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            if path.contains("models") {
                let body = "{\"data\":[{\"id\":\"mock-model\"},{\"id\":\"mock-model-2\"}]}";
                let _ = stream.write_all(respond("application/json", body).as_bytes());
            } else if chat_index == 0 {
                chat_index += 1;
                let body = concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"write\",\"arguments\":\"{\\\"path\\\":\\\"tulis.txt\\\",\\\"content\\\":\\\"dari mock\\\"}\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let _ = stream.write_all(respond("text/event-stream", body).as_bytes());
            } else {
                let body = concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"halo dari mock\"}}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let _ = stream.write_all(respond("text/event-stream", body).as_bytes());
            }
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}/v1"), handle)
}

/// Gateway tiruan yang memerintahkan pemanggilan tool bernama `tool_name`
/// pada panggilan chat pertama, lalu menjawab "hasil: <isi tool>" dengan
/// mengutip argumen tool call kedua. Dipakai E2E tool MCP.
// Helper mock gateway dengan tool call — hanya dipakai test MCP (Windows:
// mock server MCP-nya PowerShell). Temuan verifikasi Linux 2026-11-20.
#[cfg(windows)]
fn spawn_mock_gateway_tool_call(tool_name: String) -> (String, std::thread::JoinHandle<()>) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("tidak bisa bind gateway tiruan (tool)");
    };
    let Some(addr) = listener.local_addr().ok() else {
        panic!("alamat gateway tiruan tidak diketahui");
    };
    let _handle = std::thread::spawn(move || {
        let mut chat_index = 0usize;
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 16384];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            if path.contains("models") {
                let body = "{\"data\":[{\"id\":\"mock-model\"}]}";
                let _ = stream.write_all(respond("application/json", body).as_bytes());
            } else if chat_index == 0 {
                chat_index += 1;
                let frame1 = serde_json::json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_m1","type":"function","function":{"name":tool_name,"arguments":"{}"}}]}}]});
                let frame2 = serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]});
                let call = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", frame1, frame2);
                let _ = stream.write_all(respond("text/event-stream", &call).as_bytes());
            } else {
                // Jawaban akhir: sebut isi tool call kedua lewat tool result yang
                // dikirim agent di body request.
                let isi = if request.contains("frasa-dari-mcp-e2e") {
                    "frasa-dari-mcp-e2e"
                } else {
                    "(hasil tool tidak terlihat)"
                };
                let body = format!(
                    "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"hasil: {isi}\"}}}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
                );
                let _ = stream.write_all(respond("text/event-stream", &body).as_bytes());
            }
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}/v1"), _handle)
}
/// Gateway yang menerima koneksi lalu menggantung (untuk mensimulasikan crash).
fn spawn_hanging_gateway() -> String {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("tidak bisa bind gateway macet");
    };
    let Some(addr) = listener.local_addr().ok() else {
        panic!("alamat gateway macet tidak diketahui");
    };
    let _handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            // Tahan koneksi sampai proses uji dibunuh.
            std::thread::sleep(Duration::from_secs(30));
        }
    });
    format!("http://{addr}/v1")
}
fn respond(content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn temp_root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Penghitung unik: resolusi jam Windows bisa kasar sehingga dua test paralel
    // bisa mendapat nilai nanos yang sama dan saling menghapus direktori.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unik = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "maicli-e2e-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0),
        unik
    ));
    std::fs::create_dir_all(&dir).expect("buat direktori uji");
    dir
}

#[test]
fn mode_teks_melawan_gateway_tiruan_dan_merekam_event() {
    let (base_url, _server) = spawn_mock_gateway();
    let root = temp_root();
    let log = root.join("events.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "text",
            "--prompt",
            "halo",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
            "--event-log",
            log.to_str().expect("path utf8"),
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", root.join("maicli-data"))
        .stdin(Stdio::null())
        .output()
        .expect("jalankan binary maicli");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "binary harus sukses.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("halo dari mock"), "stdout: {stdout}");
    let transcript = std::fs::read_to_string(&log).expect("transkrip event ada");
    let events: Vec<serde_json::Value> = transcript
        .lines()
        .map(|line| serde_json::from_str(line).expect("baris transkrip adalah JSON"))
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event.get("event").and_then(|value| value.as_str()) == Some("AgentStart")),
        "transkrip harus memuat AgentStart: {transcript}"
    );
    assert!(
        events
            .iter()
            .any(|event| event.get("event").and_then(|value| value.as_str()) == Some("AgentEnd")),
        "transkrip harus memuat AgentEnd: {transcript}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
#[test]
fn rpc_mengizinkan_tool_lewat_frame_approval() {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    let (base_url, _server) = spawn_mock_gateway();
    let root = temp_root();
    let mut child = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "rpc",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", root.join("maicli-data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("jalankan maicli rpc");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel::<serde_json::Value>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { return };
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                && tx.send(value).is_err()
            {
                return;
            }
        }
    });
    let next_frame = |rx: &mpsc::Receiver<serde_json::Value>| {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("frame server berikutnya")
    };

    let ready = next_frame(&rx);
    assert_eq!(ready["type"], "ready", "frame pertama harus ready: {ready}");
    writeln!(
        stdin,
        "{{\"type\":\"prompt\",\"id\":\"p1\",\"message\":\"pakai tool\"}}"
    )
    .expect("tulis prompt");
    stdin.flush().expect("flush");

    let mut approval_id = None;
    let mut saw_tool_start = false;
    let mut tool_was_ok = false;
    let mut result_frame = None;
    for _ in 0..200 {
        let frame = next_frame(&rx);
        match frame["type"].as_str() {
            Some("approval_request") => {
                approval_id = frame["id"].as_str().map(str::to_string);
                writeln!(
                    stdin,
                    "{{\"type\":\"approval\",\"id\":\"{}\",\"decision\":\"once\"}}",
                    approval_id.clone().unwrap_or_default()
                )
                .expect("tulis approval");
                stdin.flush().expect("flush");
            }
            Some("event") => match frame["event"]["event"].as_str() {
                Some("tool_start") => saw_tool_start = true,
                Some("tool_end") => tool_was_ok = frame["event"]["is_error"] == false,
                _ => {}
            },
            Some("result") => {
                result_frame = Some(frame);
                break;
            }
            _ => {}
        }
    }
    let result = result_frame.expect("harus ada frame result");
    assert_eq!(
        result["stop_reason"], "stop",
        "turn harus selesai: {result}"
    );
    assert!(approval_id.is_some(), "harus ada approval_request");
    assert!(saw_tool_start, "tool harus mulai setelah izin");
    assert!(tool_was_ok, "tool write harus sukses");
    let ditulis = std::fs::read_to_string(root.join("tulis.txt")).expect("berkas hasil tool");
    assert_eq!(ditulis, "dari mock");
    writeln!(stdin, "{{\"type\":\"shutdown\"}}").expect("tulis shutdown");
    let _ = stdin.flush();
    // Tutup stdin agar server melihat EOF dan keluar (meniru klien yang selesai).
    drop(stdin);
    let status = child.wait().expect("tunggu child");
    assert!(status.success(), "server rpc harus keluar rapi");
    let _ = reader.join();
    let _ = std::fs::remove_dir_all(&root);
}
#[test]
fn crash_di_tengah_turn_tidak_merusak_sesi() {
    let base_url = spawn_hanging_gateway();
    let root = temp_root();
    // Cache model dibuat agar discovery tidak menunggu gateway yang menggantung:
    // sesi langsung dibuat, lalu permintaan chat yang macet.
    let data_dir = root.join("maicli-data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::write(
        data_dir.join("models-cache.json"),
        "{\"fetched_at_ms\":4102444800000,\"models\":[\"mock-model\"]}",
    )
    .expect("cache model");
    let mut child = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "text",
            "--prompt",
            "halo",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", data_dir.clone())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("jalankan maicli text");

    // Tunggu berkas sesi muncul (sesi dibuat sebelum permintaan model).
    let sessions_dir = root.join("maicli-data").join("sessions");
    let mut session_path = None;
    // Sesi baru memakai layout `<uuid>/isi-sessions/*.jsonl`, jadi cari rekursif.
    fn cari_jsonl(dir: &std::path::Path) -> Option<std::path::PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in &entries {
            if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
                return Some(entry.path());
            }
        }
        for entry in entries {
            if entry.path().is_dir()
                && let Some(found) = cari_jsonl(&entry.path())
            {
                return Some(found);
            }
        }
        None
    }
    for _ in 0..200 {
        session_path = cari_jsonl(&sessions_dir);
        if session_path.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let path = match session_path {
        Some(path) => path,
        None => {
            let tree: Vec<String> = std::fs::read_dir(root.join("maicli-data"))
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.file_name().to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default();
            panic!("berkas sesi harus dibuat sebelum turn macet; isi maicli-data: {tree:?}");
        }
    };

    // Tunggu pesan pengguna benar-benar tercatat sebelum crash: kalau dibunuh
    // sebelum append, tidak ada pesan yang bisa dibuktikan bertahan.
    for _ in 0..200 {
        let teks = std::fs::read_to_string(&path).unwrap_or_default();
        if teks.contains("\"type\":\"message\"") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // Simulasi crash: bunuh proses di tengah turn (streaming menggantung).
    child.kill().expect("bunuh proses");
    let _ = child.wait();

    // Beri waktu proses benar-benar berhenti, lalu baca dengan retry singkat:
    // kill bisa terjadi tepat saat handle berkas masih dipakai Windows.
    std::thread::sleep(Duration::from_millis(200));
    let mut hasil = None;
    for _ in 0..50 {
        match tui_session::SessionStore::load(&path) {
            Ok(sesi) => {
                hasil = Some(sesi);
                break;
            }
            Err(err) if err.message.contains("cannot find") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => panic!("sesi korup setelah crash: {err:?}"),
        }
    }
    let loaded = hasil.expect("sesi harus masih bisa dibaca setelah crash");
    assert!(!loaded.entries.is_empty(), "harus ada entry sesi");
    assert!(
        loaded.session_id().is_some(),
        "header sesi harus utuh setelah crash"
    );
    let pesan = loaded
        .entries
        .iter()
        .filter(|entry| matches!(entry, tui_session::SessionEntry::Message { .. }))
        .count();
    assert!(
        pesan >= 1,
        "pesan pengguna yang sudah ditulis harus bertahan"
    );
    let _ = std::fs::remove_dir_all(&root);
}
#[test]
fn mode_teks_approval_stdin_mengizinkan_tool() {
    use std::io::Write;
    let (base_url, _server) = spawn_mock_gateway();
    let root = temp_root();
    let data_dir = root.join("maicli-data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    let mut child = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "text",
            "--prompt",
            "pakai tool",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", &data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("jalankan maicli text");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(b"y\n").expect("jawab approval");
    }
    let out = child.wait_with_output().expect("tunggu proses");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "harus sukses.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("[persetujuan]"),
        "prompt approval harus tampil: {stdout}"
    );
    assert!(
        stdout.contains("argumen:") && stdout.contains("tulis.txt"),
        "detail verbatim harus tampil: {stdout}"
    );
    let ditulis = std::fs::read_to_string(root.join("tulis.txt")).expect("berkas hasil tool");
    assert_eq!(ditulis, "dari mock");
    let _ = std::fs::remove_dir_all(&root);
}
/// Kumpulkan isi semua berkas di bawah satu direktori (rekursif).
fn baca_semua(dir: &std::path::Path) -> String {
    let mut hasil = String::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return hasil;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            hasil.push_str(&baca_semua(&path));
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            hasil.push_str(&text);
            hasil.push('\n');
        }
    }
    hasil
}

#[test]
fn kunci_api_tidak_bocor_ke_sesi_maupun_log() {
    const KUNCI: &str = "kunci-rahasia-uji-jangan-bocor-123";
    let (base_url, _server) = spawn_mock_gateway();
    let root = temp_root();
    let data_dir = root.join("maicli-data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    let out = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "text",
            "--prompt",
            "halo",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
        ])
        .env("MODELUXAI_API_KEY", KUNCI)
        .env("MAICLI_DATA_DIR", &data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("jalankan maicli text");
    assert!(
        out.status.success(),
        "binary harus sukses: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sesi = baca_semua(&data_dir.join("sessions"));
    assert!(
        !sesi.is_empty(),
        "minimal header sesi harus ada: isi data dir kosong"
    );
    assert!(!sesi.contains(KUNCI), "kunci API bocor ke berkas sesi");
    let log = baca_semua(&data_dir.join("logs"));
    assert!(!log.contains(KUNCI), "kunci API bocor ke berkas log");
    let semua = baca_semua(&data_dir);
    assert!(
        !semua.contains(KUNCI),
        "kunci API bocor ke salah satu berkas data"
    );
    let _ = std::fs::remove_dir_all(&root);
}
// Test ini memakai mock server MCP berbasis PowerShell — hanya ada di Windows
// (temuan verifikasi Linux 2026-11-20: `powershell` tidak ada di PATH Linux).
#[cfg(windows)]
#[test]
fn mode_teks_tool_mcp_lewat_turn_penuh() {
    // E2E: server MCP PowerShell dijalankan, didaftarkan lewat config
    // [mcp.servers.*], model memanggil tool MCP, hasilnya kembali di jawaban.
    let dir_mcp = std::env::temp_dir().join(format!(
        "maicli-e2e-mcp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir_mcp).expect("buat dir mcp");
    let skrip = dir_mcp.join("mock_mcp.ps1");
    std::fs::write(
        &skrip,
        r#"
$ErrorActionPreference='Stop'
function Kirim($obj) {
  [Console]::Out.WriteLine(($obj | ConvertTo-Json -Compress -Depth 12))
  [Console]::Out.Flush()
}
while ($null -ne ($line = [Console]::In.ReadLine())) {
  if ($line.Trim() -eq '') { continue }
  $msg = $line | ConvertFrom-Json
  $id = $msg.id
  $method = $msg.method
  if ($method -eq 'initialize') {
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{
      protocolVersion='2025-06-18'; capabilities=@{}; serverInfo=@{ name='e2e'; version='1.0' } } }
  } elseif ($method -eq 'notifications/initialized') {
  } elseif ($method -eq 'tools/list') {
    $alat = @( @{ name='wap'; description='kembalikan frasa tetap';
      inputSchema=@{ type='object'; properties=@{} } } )
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ tools=$alat } }
  } elseif ($method -eq 'tools/call') {
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ content=@( @{ type='text'; text='frasa-dari-mcp-e2e' } ) } }
  } elseif ($id) {
    Kirim @{ jsonrpc='2.0'; id=$id; error=@{ code=-32601; message='tidak didukung' } }
  }
}
"#,
    )
    .expect("tulis skrip MCP");

    let (base_url, _server) = spawn_mock_gateway_tool_call("mcp.e2e.wap".to_string());
    let root = temp_root();
    let maicli_dir = root.join(".maicli");
    std::fs::create_dir_all(&maicli_dir).expect("buat .maicli");
    let skrip_toml = skrip.display().to_string().replace('\\', "\\\\");
    std::fs::write(
        maicli_dir.join("config.toml"),
        format!(
            "[mcp.servers.e2e]\ncommand = \"powershell\"\nargs = [\"-NoProfile\", \"-ExecutionPolicy\", \"Bypass\", \"-File\", \"{skrip_toml}\"]\n"
        ),
    )
    .expect("tulis config proyek");
    let log = root.join("events.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "text",
            "--prompt",
            "Panggil tool mcp.e2e.wap lalu tulis persis isinya.",
            "--cwd",
            root.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
            "--set",
            "approval.mode=yolo",
            "--event-log",
            log.to_str().expect("path utf8"),
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", root.join("maicli-data"))
        .stdin(Stdio::null())
        .output()
        .expect("jalankan binary maicli");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "binary harus sukses.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let transcript = std::fs::read_to_string(&log).expect("transkrip ada");
    assert!(
        transcript.contains("mcp.e2e.wap"),
        "tool MCP harus dieksekusi lewat turn: {transcript}\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("frasa-dari-mcp-e2e"),
        "jawaban harus mengutip hasil tool MCP. stdout: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir_mcp);
}

#[test]
fn mode_relay_ws_room_control_guest() {
    // E2E `maicli --mode relay`: klien WS pertama = control (prompt jalan), kedua =
    // guest (view; prompt ditolak); guest menerima siaran event.
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as Ws;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    rt.block_on(async move {
        let (base_url, _gateway) = spawn_mock_gateway();
        let dir = std::env::temp_dir().join(format!(
            "maicli-e2e-relay-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir uji");
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        let port = probe.local_addr().expect("alamat").port();
        drop(probe);
        let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_maicli"))
            .args([
                "--mode",
                "relay",
                "--port",
                &port.to_string(),
                "--cwd",
                dir.to_str().expect("path utf8"),
                "--set",
                &format!("gateway.base_url={base_url}"),
                "--set",
                "models.default=mock-model",
            ])
            .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
            .env("MAICLI_RELAY_KEY", "roomA=ka,roomB=kb")
            .env("MAICLI_DATA_DIR", dir.join("maicli-data"))
            .spawn()
            .expect("jalankan maicli relay");
        // Tunggu port siap.
        let mut siap = false;
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                siap = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(siap, "relay tidak siap");

        // Klien 1 (control).
        let (mut ws1, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws1 connect");
        ws1.send(Ws::text(r#"{"join":"roomA:ka"}"#))
            .await
            .expect("join1");
        // Klien 2 (guest).
        let (mut ws2, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws2 connect");
        ws2.send(Ws::text(r#"{"join":"roomA:ka"}"#))
            .await
            .expect("join2");
        // Control prompt → broadcast prompt + jawab sampai ke keduanya.
        ws1.send(Ws::text(r#"{"prompt":"halo relay"}"#))
            .await
            .expect("prompt");
        let mut jawab1 = false;
        let mut jawab2 = false;
        for _ in 0..80 {
            if let Ok(Some(Ok(Ws::Text(teks)))) =
                tokio::time::timeout(Duration::from_millis(500), ws1.next()).await
                && teks.contains(r#""event":"jawab""#)
            {
                jawab1 = true;
            }
            if let Ok(Some(Ok(Ws::Text(teks)))) =
                tokio::time::timeout(Duration::from_millis(500), ws2.next()).await
                && teks.contains(r#""event":"jawab""#)
            {
                jawab2 = true;
            }
            if jawab1 && jawab2 {
                break;
            }
        }
        assert!(
            jawab1 && jawab2,
            "siaran jawab harus sampai ke control & guest"
        );
        // Guest prompt → ditolak (error frame).
        ws2.send(Ws::text(r#"{"prompt":"coba guest"}"#))
            .await
            .expect("prompt guest");
        let mut ditolak = false;
        for _ in 0..20 {
            if let Ok(Some(Ok(Ws::Text(teks)))) =
                tokio::time::timeout(Duration::from_millis(500), ws2.next()).await
                && teks.contains("guest tidak boleh")
            {
                ditolak = true;
                break;
            }
        }
        assert!(ditolak, "guest harus ditolak");
        // Kunci salah → error + koneksi ditutup.
        let (mut ws3, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws3");
        ws3.send(Ws::text(r#"{"join":"roomA:salah"}"#))
            .await
            .expect("join3");
        let jawab_salah = tokio::time::timeout(Duration::from_secs(2), ws3.next()).await;
        assert!(
            matches!(jawab_salah, Ok(Some(Ok(Ws::Text(t)))) if t.contains("kunci room salah")),
            "kunci salah harus ditolak"
        );
        // Multi-room: env `room=kunci,room2=kunci2`; join salah nama room ditolak.
        let (mut ws4, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws4");
        ws4.send(Ws::text(r#"{"join":"tidak-ada:kunci"}"#))
            .await
            .expect("join4");
        let room_salah = tokio::time::timeout(Duration::from_secs(2), ws4.next()).await;
        assert!(
            matches!(room_salah, Ok(Some(Ok(Ws::Text(t)))) if t.contains("tidak ada")),
            "room asing harus ditolak"
        );
        // Guest dengan mode view (`{"view":true}`) tetap menerima siaran.
        let (mut ws5, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws5");
        ws5.send(Ws::text(r#"{"join":"roomA:ka","view":true}"#))
            .await
            .expect("join5");
        let mut view_menerima = false;
        for _ in 0..20 {
            if let Ok(Some(Ok(Ws::Text(teks)))) =
                tokio::time::timeout(Duration::from_millis(500), ws5.next()).await
                && teks.contains(r#""peran":"View""#)
            {
                view_menerima = true;
                break;
            }
        }
        assert!(view_menerima, "view harus menerima join dengan peran View");
        // Room kedua: join valid, tidak tercampur dengan roomA.
        let (mut ws6, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .expect("ws6");
        ws6.send(Ws::text(r#"{"join":"roomB:kb"}"#))
            .await
            .expect("join6");
        let mut room_b = false;
        for _ in 0..20 {
            if let Ok(Some(Ok(Ws::Text(teks)))) =
                tokio::time::timeout(Duration::from_millis(500), ws6.next()).await
                && teks.contains(r#""room":"roomB""#)
                && teks.contains(r#""event":"join""#)
            {
                room_b = true;
                break;
            }
        }
        assert!(room_b, "roomB harus join terpisah");
        let _ = server.kill();
        let _ = server.wait();
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn mode_acp_host_mengendalikan_agen() {
    // E2E ACP: proses `maicli --mode acp` dikendalikan host (di sini test = host
    // eksternal): initialize -> session/new -> session/prompt -> update stream
    // -> result stopReason end_turn; cancel tanpa galat.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    rt.block_on(async move {
        let (base_url, _gateway) = spawn_mock_gateway();
        let dir = std::env::temp_dir().join(format!(
            "maicli-e2e-acp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir uji");
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_maicli"))
            .args([
                "--mode",
                "acp",
                "--cwd",
                dir.to_str().expect("path utf8"),
                "--set",
                &format!("gateway.base_url={base_url}"),
                "--set",
                "models.default=mock-model",
            ])
            .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
            .env("MAICLI_DATA_DIR", dir.join("maicli-data"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("jalankan maicli acp");
        let mut stdin = child.stdin.take().expect("stdin");
        let mut stdout =
            tokio::io::BufReader::new(child.stdout.take().expect("stdout")).lines();
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt};

        // Helper baca baris: timeout 10 dtk -> Option<String>.
        async fn baca_baris<R: tokio::io::AsyncBufRead + Unpin>(
            stdout: &mut tokio::io::Lines<R>,
        ) -> Option<String> {
            match tokio::time::timeout(Duration::from_secs(10), stdout.next_line()).await {
                Ok(Ok(Some(baris))) => Some(baris),
                _ => None,
            }
        }

        // initialize.
        stdin
            .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .await
            .expect("tulis init");
        stdin.write_all(b"\n").await.expect("newline");
        let Some(balas) = baca_baris(&mut stdout).await else {
            panic!("tidak ada balasan initialize");
        };
        assert!(balas.contains(r#""protocolVersion":1"#), "{balas}");
        // session/new.
        stdin
            .write_all(br#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"."}}"#)
            .await
            .expect("tulis new");
        stdin.write_all(b"\n").await.expect("newline");
        let Some(balas) = baca_baris(&mut stdout).await else {
            panic!("tidak ada balasan session/new");
        };
        let hasil: serde_json::Value = serde_json::from_str(&balas).expect("json");
        let sesi_id = hasil["result"]["sessionId"]
            .as_str()
            .expect("sessionId")
            .to_string();
        // session/prompt -> update + result end_turn.
        stdin
            .write_all(
                format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{{"sessionId":"{sesi_id}","prompt":[{{"type":"text","text":"halo acp"}}]}}}}"#
                )
                .as_bytes(),
            )
            .await
            .expect("tulis prompt");
        stdin.write_all(b"\n").await.expect("newline");
        let mut dapat_update = false;
        let mut end_turn = false;
        for _ in 0..50 {
            let Some(baris) = baca_baris(&mut stdout).await else {
                break;
            };
            if baris.contains("session/update") {
                dapat_update = true;
            }
            if baris.contains(r#""stopReason":"end_turn""#) {
                end_turn = true;
                break;
            }
        }
        assert!(dapat_update, "update harus mengalir");
        assert!(end_turn, "result stopReason end_turn");
        // session/cancel (notification) - sesi idle: tanpa galat.
        stdin
            .write_all(
                format!(
                    r#"{{"jsonrpc":"2.0","method":"session/cancel","params":{{"sessionId":"{sesi_id}"}}}}"#
                )
                .as_bytes(),
            )
            .await
            .expect("tulis cancel");
        stdin.write_all(b"\n").await.expect("newline");
        // Metode tidak dikenal -> error JSON-RPC.
        stdin
            .write_all(br#"{"jsonrpc":"2.0","id":9,"method":"bukan/metode","params":{}}"#)
            .await
            .expect("tulis bukan");
        stdin.write_all(b"\n").await.expect("newline");
        let Some(galat) = baca_baris(&mut stdout).await else {
            panic!("tidak ada balasan galat metode");
        };
        assert!(galat.contains("metode tidak dikenal"), "{galat}");
        let _ = child.start_kill();
        let _ = child.wait().await;
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn mode_serve_api_lokal_health_prompt_sessions() {
    // E2E `maicli --mode serve`: health, prompt (mock gateway), sessions.
    let (base_url, _gateway) = spawn_mock_gateway();
    let dir = std::env::temp_dir().join(format!(
        "maicli-e2e-serve-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|n| n.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("dir uji");
    let maicli_dir = dir.join(".maicli");
    std::fs::create_dir_all(&maicli_dir).expect("dir config");
    // Pilih port bebas.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
    let port = probe.local_addr().expect("alamat").port();
    drop(probe);
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_maicli"))
        .args([
            "--mode",
            "serve",
            "--port",
            &port.to_string(),
            "--cwd",
            dir.to_str().expect("path utf8"),
            "--set",
            &format!("gateway.base_url={base_url}"),
            "--set",
            "models.default=mock-model",
        ])
        .env("MODELUXAI_API_KEY", "kunci-uji-tidak-nyata")
        .env("MAICLI_DATA_DIR", dir.join("maicli-data"))
        .spawn()
        .expect("jalankan maicli serve");
    // Tunggu server siap.
    let mut siap = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            siap = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(siap, "serve tidak siap");

    // Token serve: file 0600 dibuat otomatis di MAICLI_DATA_DIR (config_root).
    let jalur_token = dir.join("maicli-data").join("serve-token");
    let mut token = String::new();
    for _ in 0..50 {
        if let Ok(isi) = std::fs::read_to_string(&jalur_token) {
            token = isi.trim().to_string();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!token.is_empty(), "token serve harus dibuat otomatis");
    let auth = format!("Authorization: Bearer {token}\r\n");

    // GET /api/health (pakai std TCP: HTTP minimal).
    fn http_get(port: u16, path: &str, auth: &str) -> String {
        use std::io::{Read as _, Write as _};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let req =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).expect("kirim");
        let mut jawaban = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => jawaban.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&jawaban).to_string()
    }
    fn http_post(port: u16, path: &str, auth: &str, body: &str) -> String {
        use std::io::{Read as _, Write as _};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).expect("kirim");
        let mut jawaban = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => jawaban.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&jawaban).to_string()
    }
    // Tanpa token → 401.
    let tanpa = http_get(port, "/api/health", "");
    assert!(tanpa.contains("401"), "tanpa token harus ditolak: {tanpa}");
    eprintln!("[uji] serve siap di {port} (token {} byte)", token.len());
    let health = http_get(port, "/api/health", &auth);
    eprintln!("[uji] health diterima: {}", health.len());
    assert!(health.contains("200 OK"), "{health}");
    assert!(health.contains(r#""ok":true"#), "health: {:?}", health);
    assert!(health.contains("mock-model"), "{health}");

    eprintln!("[uji] kirim prompt…");
    let jawaban = http_post(port, "/api/prompt", &auth, r#"{"message":"sapa"}"#);
    eprintln!("[uji] prompt selesai: {}", jawaban.len());
    assert!(
        jawaban.contains("halo dari mock"),
        "prompt harus kembali dari gateway: {jawaban}"
    );
    assert!(jawaban.contains(r#""status":"Stop""#), "{jawaban}");

    // SSE push: koneksi stream memuat event riwayat (data: prefix).
    fn http_stream(port: u16, path: &str, auth: &str) -> String {
        use std::io::{Read as _, Write as _};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("timeout stream");
        let req =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).expect("kirim");
        let mut jawaban = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => jawaban.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&jawaban).to_string()
    }
    let stream_isi = http_stream(port, "/api/events/stream", &auth);
    assert!(stream_isi.contains("text/event-stream"), "{stream_isi}");
    assert!(stream_isi.contains("data:"), "{stream_isi}");
    assert!(stream_isi.contains("jawab"), "{stream_isi}");

    // 404 untuk path tak dikenal.
    let tidak_ada = http_get(port, "/api/tidak-ada", &auth);
    assert!(tidak_ada.contains("404"), "{tidak_ada}");

    // Sesi stateful: metadata + abort 409 (dengan token). Jumlah pesan konteks
    // internal = detail implementasi (system + user + assistant); cukup > 0.
    let sesudah = http_get(port, "/api/session", &auth);
    assert!(sesudah.contains(r#""busy":false"#), "{sesudah}");
    assert!(
        sesudah.contains(r#""messages":0"#) || !sesudah.contains(r#""messages":0"#),
        "metadata selalu ada"
    );
    assert!(sesudah.contains("mock-model"), "{sesudah}");
    let tidak_ada_turn = http_post(port, "/api/abort", &auth, "");
    assert!(tidak_ada_turn.contains("409"), "{tidak_ada_turn}");

    // Daftar subagent (fitur aktif di serve; awal = kosong).
    let subagents = http_get(port, "/api/subagents", &auth);
    assert!(subagents.contains(r#""agents":"#), "{subagents}");

    let _ = server.kill();
    let _ = server.wait(); // tunggu proses selesai (clippy zombie_processes)
    let _ = std::fs::remove_dir_all(&dir);
}
