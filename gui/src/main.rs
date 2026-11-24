//! Entry point aplikasi desktop maicli.

fn main() {
    if let Err(err) = maicli_gui_lib::run() {
        eprintln!("[maicli] gagal menjalankan GUI: {err}");
        std::process::exit(1);
    }
}
