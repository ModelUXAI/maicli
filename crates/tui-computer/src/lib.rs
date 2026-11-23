//! Tool `computer`: screenshot dan input desktop.
//!
//! Tier Exec: setiap pemanggilan lewat approval. Screenshot Linux memakai X11
//! (`x11rb`); Windows/macOS memakai crate `xcap`. Input memakai `enigo`.
//! Lingkungan tanpa display menghasilkan error bernama, bukan panic.

use std::path::PathBuf;

use async_trait::async_trait;
use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings,
};
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use tui_tools::{Tool, ToolContext};

/// Tool kontrol desktop.
#[derive(Debug, Default, Clone, Copy)]
pub struct ComputerTool;

fn gagal_enigo(err: impl std::fmt::Display) -> String {
    format!("input desktop gagal: {err}")
}

fn parse_button(nama: Option<&str>) -> Result<Button, String> {
    match nama.unwrap_or("left") {
        "left" | "kiri" => Ok(Button::Left),
        "right" | "kanan" => Ok(Button::Right),
        "middle" | "tengah" => Ok(Button::Middle),
        lain => Err(format!("tombol '{lain}' tidak dikenal (left|right|middle)")),
    }
}

fn key_dari(nama: &str) -> Result<Key, String> {
    let lower = nama.trim().to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "space" | "spasi" => Key::Space,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        _ => {
            let mut chars = nama.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Unicode(c),
                _ => return Err(format!("key '{nama}' tidak dikenal")),
            }
        }
    };
    Ok(key)
}

fn modifier(nama: &str) -> Option<Key> {
    match nama.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(Key::Control),
        "alt" | "option" => Some(Key::Alt),
        "shift" => Some(Key::Shift),
        "meta" | "cmd" | "super" => Some(Key::Meta),
        _ => None,
    }
}

/// Kirim satu chord seperti `ctrl+c` atau `enter`.
fn tekan_chord(enigo: &mut Enigo, chord: &str) -> Result<(), String> {
    let bagian: Vec<&str> = chord.split('+').filter(|s| !s.is_empty()).collect();
    if bagian.is_empty() {
        return Err("chord kosong".to_string());
    }
    let (terakhir, modifiers) = bagian.split_last().ok_or("chord kosong")?;
    let mut ditahan = Vec::new();
    for nama in modifiers {
        let Some(key) = modifier(nama) else {
            return Err(format!("modifier '{nama}' tidak dikenal"));
        };
        enigo.key(key, Direction::Press).map_err(gagal_enigo)?;
        ditahan.push(key);
    }
    let hasil = key_dari(terakhir).and_then(|key| {
        enigo
            .key(key, Direction::Click)
            .map_err(gagal_enigo)
            .map(|_| ())
    });
    for key in ditahan.into_iter().rev() {
        let _ = enigo.key(key, Direction::Release);
    }
    hasil
}

/// Ambil screenshot ke `path`.
#[cfg(target_os = "linux")]
fn screenshot(path: &PathBuf) -> Result<(u32, u32), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("tidak bisa konek ke display X11: {e}"))?;
    let screen = &conn.setup().roots[screen_num];
    let geometri = conn
        .get_geometry(screen.root)
        .map_err(|e| format!("get_geometry: {e}"))?
        .reply()
        .map_err(|e| format!("get_geometry: {e}"))?;
    let gambar = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            screen.root,
            0,
            0,
            geometri.width,
            geometri.height,
            !0,
        )
        .map_err(|e| format!("get_image: {e}"))?
        .reply()
        .map_err(|e| format!("get_image: {e}"))?;
    let bpp = u32::from(
        conn.setup()
            .pixmap_formats
            .first()
            .map(|f| f.bits_per_pixel)
            .unwrap_or(32),
    );
    let mut rgba = Vec::with_capacity((geometri.width as usize) * (geometri.height as usize) * 4);
    if bpp == 32 {
        for pixel in gambar.data.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    } else {
        for pixel in gambar.data.as_chunks::<3>().0 {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    let img = image::RgbaImage::from_raw(geometri.width.into(), geometri.height.into(), rgba)
        .ok_or_else(|| "buffer piksel tidak sesuai ukuran layar".to_string())?;
    img.save(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((geometri.width.into(), geometri.height.into()))
}

#[cfg(not(target_os = "linux"))]
fn screenshot(path: &PathBuf) -> Result<(u32, u32), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("monitor tidak terbaca: {e}"))?;
    let monitor = monitors
        .first()
        .ok_or_else(|| "tidak ada monitor".to_string())?;
    let gambar = monitor
        .capture_image()
        .map_err(|e| format!("screenshot gagal: {e}"))?;
    let (width, height) = (gambar.width(), gambar.height());
    gambar
        .save(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((width, height))
}

fn screenshot_path(ctx: &ToolContext, raw: Option<&str>) -> Result<PathBuf, String> {
    let nama = match raw {
        Some(path) if !path.trim().is_empty() => path.trim().to_string(),
        _ => format!("screenshot-{}.png", std::process::id()),
    };
    tui_tools::resolve_in_cwd(&ctx.cwd, &nama)
        .map_err(|err| format!("{}: {}", err.code, err.message))
}

/// Daftar jendela X11 (Linux); platform lain belum didukung.
#[cfg(target_os = "linux")]
fn daftar_jendela() -> Result<String, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, MapState};
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("tidak bisa konek ke display X11: {e}"))?;
    let layar = &conn.setup().roots[screen_num];
    let pohon = conn
        .query_tree(layar.root)
        .map_err(|e| format!("query_tree: {e}"))?
        .reply()
        .map_err(|e| format!("query_tree: {e}"))?;
    let mut baris = Vec::new();
    for id in pohon.children.into_iter().take(40) {
        let Ok(attr) = conn.get_window_attributes(id).map_err(|e| e.to_string()) else {
            continue;
        };
        let Ok(attr) = attr.reply() else { continue };
        if attr.map_state != MapState::VIEWABLE {
            continue;
        }
        let nama = conn
            .get_property(
                false,
                id,
                x11rb::protocol::xproto::AtomEnum::WM_NAME,
                0u32,
                0u32,
                256u32,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .unwrap_or_default();
        if !nama.trim().is_empty() {
            baris.push(format!("0x{id:x} · {}", nama.trim()));
        }
        if baris.len() >= 20 {
            break;
        }
    }
    Ok(if baris.is_empty() {
        "tidak ada jendela terlihat".to_string()
    } else {
        baris.join("\n")
    })
}

#[cfg(not(target_os = "linux"))]
fn daftar_jendela() -> Result<String, String> {
    Err("daftar jendela belum didukung di platform ini".to_string())
}

/// Tree aksesibilitas sederhana: jendela + geometri (X11).
#[cfg(target_os = "linux")]
fn ax_tree() -> Result<String, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, MapState};
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("tidak bisa konek ke display X11: {e}"))?;
    let layar = &conn.setup().roots[screen_num];
    let pohon = conn
        .query_tree(layar.root)
        .map_err(|e| format!("query_tree: {e}"))?
        .reply()
        .map_err(|e| format!("query_tree: {e}"))?;
    let mut baris = Vec::new();
    for id in pohon.children.into_iter().take(80) {
        let Ok(attr) = conn.get_window_attributes(id).map_err(|e| e.to_string()) else {
            continue;
        };
        let Ok(attr) = attr.reply() else { continue };
        if attr.map_state != MapState::VIEWABLE {
            continue;
        }
        let nama = conn
            .get_property(
                false,
                id,
                x11rb::protocol::xproto::AtomEnum::WM_NAME,
                0u32,
                0u32,
                256u32,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .unwrap_or_default();
        if nama.trim().is_empty() {
            continue;
        }
        let geo = conn
            .get_geometry(id)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|g| (g.x, g.y, g.width, g.height))
            .unwrap_or((0, 0, 0, 0));
        baris.push(format!(
            "window 0x{id:x} \"{}\" {}x{}+{}+{}",
            nama.trim(),
            geo.2,
            geo.3,
            geo.0,
            geo.1
        ));
        if baris.len() >= 20 {
            break;
        }
    }
    Ok(if baris.is_empty() {
        "tidak ada elemen aksesibilitas".to_string()
    } else {
        baris.join("\n")
    })
}

#[cfg(not(target_os = "linux"))]
fn ax_tree() -> Result<String, String> {
    Err("AX tree belum didukung di platform ini".to_string())
}

fn clipboard_baca() -> Result<String, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))?;
    cb.get_text().map_err(|e| format!("clipboard baca: {e}"))
}

fn clipboard_tulis(teks: &str) -> Result<String, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))?;
    cb.set_text(teks.to_string())
        .map_err(|e| format!("clipboard tulis: {e}"))
        .map(|_| format!("clipboard: {} karakter", teks.chars().count()))
}

fn aksi(action: &str, args: &Value, ctx: &ToolContext) -> Result<String, String> {
    match action {
        "status" => {
            let display = std::env::var("DISPLAY").unwrap_or_default();
            let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
            let enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            let (x, y) = enigo.location().map_err(gagal_enigo)?;
            Ok(format!(
                "computer siap: display='{display}' wayland='{wayland}' kursor=({x},{y})"
            ))
        }
        "screenshot" => {
            let path = screenshot_path(ctx, args.get("path").and_then(Value::as_str))?;
            screenshot(&path).map(|(w, h)| format!("screenshot {w}x{h}: {}", path.display()))
        }
        "move" => {
            let Some(x) = args.get("x").and_then(Value::as_i64) else {
                return Err("action=move butuh x".to_string());
            };
            let Some(y) = args.get("y").and_then(Value::as_i64) else {
                return Err("action=move butuh y".to_string());
            };
            let relative = args
                .get("relative")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            enigo
                .move_mouse(
                    x as i32,
                    y as i32,
                    if relative {
                        Coordinate::Rel
                    } else {
                        Coordinate::Abs
                    },
                )
                .map_err(gagal_enigo)
                .map(|_| format!("mouse → ({x}, {y})"))
        }
        "click" => {
            let button = parse_button(args.get("button").and_then(Value::as_str))?;
            let count = args
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 5);
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            if let (Some(x), Some(y)) = (
                args.get("x").and_then(Value::as_i64),
                args.get("y").and_then(Value::as_i64),
            ) {
                enigo
                    .move_mouse(x as i32, y as i32, Coordinate::Abs)
                    .map_err(gagal_enigo)?;
            }
            for _ in 0..count {
                enigo
                    .button(button, Direction::Click)
                    .map_err(gagal_enigo)?;
            }
            Ok(format!("klik {button:?} × {count}"))
        }
        "scroll" => {
            let dy = args.get("dy").and_then(Value::as_i64).unwrap_or(0);
            let dx = args.get("dx").and_then(Value::as_i64).unwrap_or(0);
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            if dy != 0 {
                enigo
                    .scroll(dy as i32, Axis::Vertical)
                    .map_err(gagal_enigo)?;
            }
            if dx != 0 {
                enigo
                    .scroll(dx as i32, Axis::Horizontal)
                    .map_err(gagal_enigo)?;
            }
            Ok(format!("scroll dx={dx} dy={dy}"))
        }
        "type" => {
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return Err("action=type butuh text".to_string());
            };
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            enigo.text(text).map_err(gagal_enigo)?;
            Ok(format!("mengetik {} karakter", text.chars().count()))
        }
        "windows" => daftar_jendela(),
        "ax" => ax_tree(),
        "clipboard_read" => clipboard_baca(),
        "clipboard_write" => {
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return Err("action=clipboard_write butuh text".to_string());
            };
            clipboard_tulis(text)
        }
        "key" => {
            let Some(keys) = args.get("keys").and_then(Value::as_str) else {
                return Err("action=key butuh keys, mis. ctrl+c".to_string());
            };
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(gagal_enigo)?;
            tekan_chord(&mut enigo, keys).map(|_| format!("tombol: {keys}"))
        }
        "run" => {
            let steps = args
                .get("steps")
                .and_then(Value::as_array)
                .ok_or_else(|| "action=run butuh array steps".to_string())?;
            if steps.is_empty() || steps.len() > 32 {
                return Err("steps harus 1..32".to_string());
            }
            let mut hasil = Vec::new();
            for (i, step) in steps.iter().enumerate() {
                let aksi = step
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("langkah {} tanpa action", i + 1))?;
                if aksi == "run" {
                    return Err(format!("langkah {}: run bersarang ditolak", i + 1));
                }
                match self::aksi(aksi, step, ctx) {
                    Ok(teks) => hasil.push(format!("{}. {}", i + 1, teks)),
                    Err(err) => {
                        hasil.push(format!("{}. gagal: {err}", i + 1));
                        return Ok(hasil.join("\n"));
                    }
                }
            }
            Ok(hasil.join("\n"))
        }
        lain => Err(format!("action '{lain}' tidak dikenal")),
    }
}

#[async_trait]
impl Tool for ComputerTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "computer".to_string(),
            description: "Kontrol desktop: screenshot, gerakkan/klik mouse, scroll, ketik teks, \
                          dan tekan tombol. Lingkungan tanpa display akan menolak dengan pesan jelas."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "screenshot", "move", "click", "scroll", "type", "key", "windows", "ax", "clipboard_read", "clipboard_write", "run"]
                    },
                    "path": { "type": "string" },
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "relative": { "type": "boolean" },
                    "button": { "type": "string" },
                    "count": { "type": "integer", "minimum": 1, "maximum": 5 },
                    "dx": { "type": "integer" },
                    "dy": { "type": "integer" },
                    "text": { "type": "string" },
                    "keys": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["action"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    async fn execute(
        &self,
        args: &Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, tui_wire::MxError> {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("status");
        let hasil = aksi(action, args, ctx);
        Ok(match hasil {
            Ok(text) => ToolOutput::ok(text),
            Err(err) => ToolOutput::err(err),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn tombol_dikenal_dan_asing_ditolak() {
        assert_eq!(parse_button(Some("right")).unwrap(), Button::Right);
        assert!(parse_button(Some("tengah")).is_ok());
        assert!(parse_button(Some("x")).is_err());
    }

    #[test]
    fn key_tunggal_dan_nama_dikenal() {
        assert!(matches!(key_dari("a").unwrap(), Key::Unicode('a')));
        assert!(matches!(key_dari("enter").unwrap(), Key::Return));
        assert!(key_dari("dua-kata").is_err());
    }

    #[test]
    fn path_screenshot_dibatasi_cwd() {
        let ctx = ToolContext::new(std::env::temp_dir());
        assert!(screenshot_path(&ctx, Some("../luar.png")).is_err());
        assert!(screenshot_path(&ctx, Some("x.png")).is_ok());
    }
    #[test]
    fn clipboard_roundtrip_bila_tersedia() {
        let teks = format!("maicli-clip-{}", std::process::id());
        if clipboard_tulis(&teks).is_err() {
            return; // lingkungan tanpa clipboard
        }
        assert_eq!(clipboard_baca().unwrap_or_default(), teks);
    }
    #[tokio::test]
    async fn run_menolak_steps_kosong() {
        let tool = ComputerTool;
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(&json!({"action":"run","steps":[]}), &ctx)
            .await
            .expect("tool jalan");
        assert!(out.is_error);
        assert!(out.content.contains("1..32"), "{}", out.content);
    }
    #[test]
    fn ax_tree_mengembalikan_hasil_atau_error_jelas() {
        match ax_tree() {
            Ok(teks) => assert!(teks.contains("window") || teks.contains("tidak ada")),
            Err(err) => assert!(err.contains("X11") || err.contains("belum didukung")),
        }
    }
}
