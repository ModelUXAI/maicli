# docs/paritas.md — ledger bukti perilaku

> **Keputusan iqbal (2026-11-18): target paritas dengan omp DILEPAS** (opsi B).
> Produk berjalan pada arah sendiri (ModeluXAI bawaan, registry provider sendiri,
> integrasi RPC/MCP versi sendiri). Ledger ini kini mengukur **bukti perilaku
> internal** (test/golden/frame emas) — bukan pembandingan dengan omp.
> Checkout `oh-my-pi-main/` tetap sebagai rujukan sejarah perilaku di komentar
> kode, tanpa status ketergantungan.

Aturan: setiap baris wajib punya **bukti yang bisa dijalankan** (test /
transkrip golden / frame emas). Baris tanpa bukti = `belum`. Status legacy
`port` dibaca sebagai "di-port dari omp + diuji internal".

Dibuat 2026-11-17 (F-12); diperbarui 2026-11-18 (keputusan lepas paritas omp; C-12 ditutup dengan golden internal + uji mutasi).

## Ringkasan

| Status | Jumlah |
|---|---|
| port — di-port dari omp + diuji internal | 153 (1 khusus Windows) |
| port (sebagian) — protokol ada, wiring sebagian | 0 |
| BLOCKED | 0 |
| di luar v1 / ditutup oleh keputusan | 3 — golden omp (F-01, ditutup: paritas dilepas), migrasi sesi lama (tidak perlu), impor sesi omp v3 (Fase 2) |

## v1 (M1)

| Fitur | Status | Bukti |
|---|---|---|
| `content_filter` → `stopReason` error (tanpa retry) | port | `client::tests::content_filter_dipetakan_ke_error_tanpa_retry` |
| SSE tahan chunk terpotong | port | `tui-provider` sse::tests (8, termasuk CRLF lintas chunk dan payload kosong) |
| Perakitan tool_call terfragmentasi | port | `wire::tests::tool_call_terfragmentasi_dirakit` |
| Pemetaan finish_reason → StopReason | port | `wire::tests::{length_dipetakan,teks_dan_finish_stop}` |
| Retry 429/5xx + jitter | port | `client::tests::discovery_retry_setelah_429_lalu_sukses`, `jeda_retry_dalam_batas` |
| Error taxonomy stabil | port | `tui-wire` error::tests (3) |
| Discovery `/v1/models` | port | `client::tests::discovery_membaca_daftar_model` |
| Resolusi model (`match_model`) | port | `models::tests` (6: persis, case, prefiks, substring, ambigu, kosong) |
| Cache discovery + TTL | port | `models::tests::cache_menghormati_ttl` |
| Loop dua tingkat + antrean ganda | port | `tui-core` agent::tests (4) |
| Aturan stopReason (`stop`≡`toolUse`) | port | `turn_length_tidak_menjalankan_tool…`, `tool_call_runnable_dieksekusi…` |
| Deadline turn menghentikan tool yang berjalan | port | `agent::tests::deadline_membatalkan_tool_yang_sedang_berjalan`; penjaga deadline memicu cancel |
| Deadline turn | port | `TurnSettings.deadline` + cek di loop |
| Abort turn dari UI (Esc) | port | `core::agent::tests::turn_yang_dibatalkan_melepas_penanda_sibuk` + `tui_mode::tests::escape_saat_busy_mengirim_sinyal_batal` |
| Pause gate (`/pause`, `/resume`) | port | `pause_menolak_turn_berikutnya`, `pause_di_tengah_turn_berhenti_sebelum_panggilan_model_berikut`, `commands::tests::pause_dan_resume_dikenali` |
| Lock sesi lintas-proses | port | `lock_kedua_ditolak_saat_sesi_masih_dipakai`, `lock_dilepas_saat_store_di_drop`, `lock_basi_diambil_alih` |
| Sinyal abort sampai ke tool | port | `CancelToken` + `tools::tests::bash_berhenti_saat_dibatalkan` (< 1 dtk) |
| Abort saat streaming model | port | `agent::tests::cancel_menghentikan_turn_yang_menunggu_stream` |
| Status line memakai `TurnStatus` §7.2 (streaming/tool/terminal) | port | `tui_mode::tests::status_line_mengikuti_turn_status` |
| Status line memuat model, konteks, mode approval, status turn (C-09) | port | `tui_mode::tests::status_line_menyebut_mode_approval`, `status_menampilkan_persen_konteks`, `status_menyebut_model_dan_keadaan` |
| TUI: bantuan F1 memuat semua tombol + semantik Ctrl+C | port | `tui_mode::tests::bantuan_menyebut_semua_tombol_penting`, `hint_saat_sibuk_menyebut_pembatalan` |
| TUI: gulir histori + overlay bantuan | port | `tui_mode::tests` (5: scroll clamp, PageUp, F1, baris baru, status) |
| TUI: gulir sadar word-wrap | port | `gulir_memakai_baris_visual_setelah_lipat` |
| TUI: detail tool call (Ctrl+E) | port | `ctrl_e_menampilkan_detail_tool_lalu_meringkas_balik` |
| TUI: palet command `Ctrl+K` | port | `ctrl_k_membuka_palet_dan_menjalankan_command`, `palet_command_berargumen_mengisi_composer`, `palet_dibatalkan_dengan_escape` |
| TUI: composer multi-baris | port | `shift_enter_menambah_baris_baru_tanpa_mengirim`, `tinggi_composer_mengikuti_jumlah_baris` |
| Logging ke berkas + instrumentasi frame/delta | port | `logging::tests::log_ditulis_ke_berkas_dan_bisa_dibaca_ulang` |
| TUI: `Ctrl+C` → batal dulu, keluar dengan konfirmasi | port | `ctrl_c_saat_sibuk_membatalkan_turn_dulu`, `ctrl_c_dua_kali_saat_idle_baru_keluar` |
| TUI: riwayat prompt (↑/↓) | port | `riwayat_prompt_dipanggil_dengan_panah`, `panah_tetap_menggulir_saat_composer_berisi` |
| TUI: tempel teks (paste) | port | `paste_baris_baru_menjadi_spasi_dan_dibatasi`, `ctrl_u_menghapus_composer` |
| TUI: tabel markdown | port | `tabel_markdown_dirender_dengan_lebar_seragam`, `tabel_hanya_dikenali_bila_ada_baris_pemisah` |
| TUI: blok kode berpagar | port | `blok_kode_berpagar_tidak_dinilai_sebagai_teks_biasa`, `baris_streaming_di_dalam_blok_kode_tetap_redup` |
| TUI: penekanan ringan (heading/tebal/kode) | port | `teks_histori_mendukung_tebal_dan_kode`, `penanda_tidak_ditutup_dibiarkan_apa_adanya`, `heading_memakai_warna_aksen_dan_tebal` |
| TUI: indikator konteks | port | `tui_mode::tests::status_menampilkan_persen_konteks` (window 0 tidak membagi nol) |
| TUI: pemilih sesi | port | `tui_mode::tests::daftar_sesi_membuka_pemilih`, `daftar_sesi_kosong_menjadi_pesan`, `pemilih_sesi_dinavigasi_dan_dipilih`, `pemilih_sesi_dibatalkan_dengan_escape` |
| TUI: roda mouse | port | `roda_mouse_menggulir_dan_di_clamp` |
| TUI: lebar teks CJK/emoji | port | `lebar_teks_menghitung_cjk_dan_emoji` |
| TUI: tema warna dari config | port | `parse_color_menerima_dua_bentuk_hex`, `ui_hex_tidak_valid_ditolak`, `ui_default_tampil_di_describe` |
| TUI: dialog approval inline | port | `approval_memakai_tombol_y_n_dan_tidak_menutup_sendiri` |
| Event dua tingkat | port | `event_done_membawa_pesan` |
| Tool read/write/edit/glob/grep/bash/todo/ask | port | `tui-tools` tests (25) |
| `read` offset/limit + tolak biner | port | `read_menghormati_offset_dan_limit`, `read_menolak_berkas_biner`, `read_offset_di_luar_isi_memberi_keterangan` |
| `grep` batas hasil | port | `grep_menghormati_max_results` |
| `ask` jujur tanpa UI | port | `ask_interaktif_tetap_jujur_belum_ada_ui` (mode tanpa kanal) |
| `ask` bertanya di TUI | port | `ask_mengirim_pertanyaan_dan_menerima_jawaban`, `ask_jawaban_kosong_dianggap_dilewati`, `pertanyaan_dijawab_lewat_enter_tanpa_mengirim_prompt` |
| `ask` bertanya di mode teks | port | `route_line_menjawab_pertanyaan_aktif`, `route_line_menyimpan_baris_tanpa_pertanyaan`, `route_line_jawaban_kosong_dilewati_dan_kedaluwarsa_dicatat` |
| Kill-tree bash saat timeout | port | `bash_timeout_menghentikan_perintah` (suite 0,75 dtk) |
| Path traversal ditolak | port | `read_menolak_traversal` |
| Cap output tool | port | `output_dipotong_sesuai_cap` |
| Edit: cocok unik + replace_all | port | `edit_*` (5 test) |
| Approval 4 mode × 3 tier | port | `approval::tests::matriks_mode_kali_tier` |
| Folder terlarang | port | `folder_terlarang_selalu_bertanya` |
| Grant sesi | port | `grant_sesi_hanya_berlaku_untuk_subjek_sama` |
| Audit approval di sesi | port | `audit_approval_tercatat_untuk_izin_dan_tolak`, `approval_serialisasi_dengan_tier`; app menulis entry `approval_decision` di mode teks/TUI/RPC |
| Sesi JSONL append + resume | port | `tui-session` store tests (5) + `resume_hanya_mengambil_entry_pesan` |
| Daftar & pemilihan sesi | port | `list_menghitung_pesan…`, `latest_mengikuti_file_terakhir…`, `file_rusak_dilewati…` |
| Resume lewat CLI | port | `--resume [ID]`, `--list-sessions`; core test `pesan_resume_ikut_pada_permintaan_pertama` |
| Fork `parent_session` | port | `fork_menyetel_parent_session` |
| Tulis atomik | port | `write_atomic` + round-trip |
| Impor sesi omp v3 | di luar v1 | M4/Fase 2 — bukan scope v1 (RENCANA §13) |
| Config berlapis + provenance | port | `tui-config` tests (4) |
| Redaksi kunci | port | `kunci_tidak_akan_ditampilkan` |
| Slash command v1 | port | `tui-core` commands::tests (6) |
| TUI viewport + composer + streaming | port | `tui_mode.rs` (belum ada frame emas) |
| Dialog approval di TUI | port | `handle_key` (y/1/2/3/n) |
| Estimasi token + `Budget` | port | `tui-context` tokens::tests + budget::tests |
| Pruning hasil tool lama | port | `prune::tests` (4) |
| Rencana + penerapan compaction | port | `plan::tests` (6) + core `compaction_*` (3) |
| Ringkasan compaction dipersist + dipakai resume | port | session `resume_menerapkan_compaction`; app menulis entry `compaction` di mode teks/TUI/RPC |
| Pemicu compaction = max(estimasi, usage provider) + hysteresis | port | `agent::tests::compaction_dipicu_usage_provider_yang_lebih_besar`, `hysteresis_mencegah_compaction_berulang` |
| Compaction `shake` (tanpa LLM) | port | `shake::tests::shake_memangkas_hasil_lama_dan_mempertahankan_terbaru`, `shake_tidak_menyentuh_pesan_biasa`, `agent::tests::shake_dipakai_tanpa_panggilan_model_saat_hasil_tool_lama_besar`, `shake_gagal_berarti_fallback_ke_soft` |
| Compaction manual `/compact` + laporan token | port | `agent::tests::compact_now_mengembalikan_laporan_token`, `compact_now_tanpa_yang_bisa_diringkas_mengembalikan_none`, `commands::tests::compaction_menyebut_token_dan_jumlah_pesan` |
| Pemakaian token + `/usage` | port | `usage_terakumulasi_dengan_aman` (wire), `usage_terakumulasi_antar_turn`, `usage_dipisah_per_model`, `usage_dikenali_dan_diformat`, `usage_merinci_per_model_bila_lebih_dari_satu` |
| RPC `usage` | port | `rpc::tests::usage_request_dan_frame` |
| Sesi: hapus (`dropSession`) | port | `store::tests::remove_menghapus_berkas_dan_lock`, `remove_menolak_sesi_yang_sedang_dipakai`, `remove_sesi_tidak_ada_dilaporkan` |
| Sesi: pencarian judul (C-06) | port | `store::tests::search_judul_tanpa_peduli_huruf_besar`, `commands::tests::sessions_dikenali` |
| Skema entry sesi ketat (field asing ditolak) | port | `store::tests::entry_dengan_field_asing_ditolak`; hanya EOF yang dimaafkan di baris terakhir |
| Penjagaan versi skema sesi (versi lebih baru ditolak) | port | `store::tests::header_versi_baru_ditolak_dengan_pesan_jelas`, `header_versi_nol_ditolak`; kebijakan di `docs/skema-sesi-v1.md` |
| Sesi: batas ukuran + rotasi (C-06) | port | `store::tests::rotasi_sesi_saat_batas_ukuran_tercapai`, `tanpa_rotasi_bila_di_bawah_batas` |
| Sesi: fork (`mx --fork`) | port | `store::tests::fork_menyetel_parent_session`, `main::tests::fork_membuat_salinan_dengan_pesan_dan_model`, `fork_sesi_tidak_ada_dilaporkan` |
| Sesi: gagal buka/buat dilaporkan | port | `session_open::tests::direktori_tak_bisa_dibuat_dilaporkan`, `resume_ke_file_tidak_ada_dilaporkan`, `sesi_baru_dibuat_di_direktori_bersih` |
| Slash `/sessions` (daftar sesi) | port | `commands::tests::sessions_dikenali`; teks & TUI memakai `tui_session::list` |
| Slash `/switch [id]` (pindah sesi) | port | `commands::tests::switch_sesi_dikenali`, `store::tests::resolve_target_latest_dan_id_persis`, `match_session_persis_prefix_dan_ambigu`, `tui_mode::tests::pindah_sesi_mengosongkan_histori` |
| `fitOutputTokens` (max_tokens menyesuaikan sisa window) | port | `output_tokens_mengikuti_sisa_window`, `output_tokens_dipaksa_minimum_saat_window_penuh`, `output_tokens_tanpa_setting_tetap_kosong` |
| Perintah ber-badge di timeline (C-08) | port | `tui_mode::tests::label_timeline_membedakan_perintah_dan_prompt`, `label_timeline_dipakai_saat_switch` |
| Registry slash command beratribut (C-08) | port | `registry_memiliki_atribut_dan_alias`, `command_tanpa_argumen_menolak_argumen`, `help_menyebut_semua_command` |
| Banner peringatan mode `yolo` (§10.11) | port | `banner::tests::banner_menyebut_yolo_dan_tanpa_persetujuan`, `tui_mode::tests::banner_yolo_hanya_untuk_mode_yolo` |
| ACL file sensitif → pengguna saja (§10.9) | port (Windows) | `secure_file::tests::restrict_menghasilkan_acl_pengguna_saja`, `restrict_ulang_tetap_valid`, `file_tidak_ada_dilaporkan_error`; `store::tests::file_sesi_dibatasi_ke_pengguna_saja`; `grants::tests::file_grant_dibatasi_ke_pengguna_saja`; `config` ACL test |
| Cache daftar model di file (C-02) | port | `models_cache::tests::simpan_dan_muat_bolak_balik`, `cache_kedaluwarsa_diabaikan`, `file_rusak_atau_kosong_dianggap_tidak_ada` |
| `/model` discovery-aware | port | `commands::tests::model_tanpa_arg_menampilkan_daftar_bila_ada`, `model_tidak_dikenal_ditolak_tanpa_menebak`, `model_tidak_dikenal_lewat_dispatch_tidak_mengganti` |
| Slash `/new` dan `/fork` (in-app) | port | `commands::tests::new_dan_fork_dikenali`, `session_open::tests::create_new_membuat_sesi_baru`, `fork_current_menyalin_pesan` |
| Slash `/context` dan `/todo` | port | `commands::tests::context_dan_todo_dikenali`, `context_dan_todo_diformat`, `tools::tests::registry_membagikan_daftar_todo` |
| Slash `/title <teks>` | port | `commands::tests::title_sesi_dikenali`, `store::tests::title_change_menang_atas_header` |
| Model per sesi (`ModelChange`) | port | `store::tests::model_change_terakhir_terbaca_dan_tertua_diabaikan`; dipakai saat resume (`--model` > sesi > config) |
| Pemakaian token per sesi (`ModelUsage`) | port | `message::tests::delta_dan_nol_pada_usage`, `store::tests::usage_dijumlahkan_dari_semua_entry`; `/usage` setelah resume menjumlahkan sesi (delta per turn) |
| `mx config set <key> <value>` | port | `set_nilai_tersimpan_dan_terbaca`, `set_mempertahankan_key_lain`, `set_menolak_key_asing_dan_nilai_salah`, `set_angka_menjadi_integer` |
| Kunci API dari keyring OS — **jalur Windows nyata dijalankan** (key-import + baca ulang tanpa env) | port | `V-02-gateway.md` butir 5; `mx config key-import` → Credential Manager `mx-modeluxai` | `secret::tests::env_menang_atas_keyring`, `keyring_dipakai_bila_env_kosong_atau_spasi`, `store_menyimpan_lewat_backend`, `store_menolak_kunci_kosong_dan_meneruskan_galat`, `tests::sumber_kunci_dilaporkan_tanpa_nilai` |
| Registry provider + `/provider` (ModeluXAI + katalog omp) | port | `tui_wire::providers::tests::*` (6), `tui_config::tests::provider_*` (4), `commands::tests::provider_*` (2) |
| Subagent lokal (tool + runner sesi anak) | port (tahap A) | `tui_subagent::tests::subagent_satu_turn_mengembalikan_teks`, `subagent_menjalankan_tool_dan_melanjutkan`, `tool_subagent_eksekusi_menghasilkan_ringkasan_dan_teks`; `docs/subagent-v1.md` |
| Collab tamu polling riwayat (GET /api/events) | port | E2E `mode_serve_api_lokal_health_prompt_sessions` (assert events) |
| API server lokal (mx serve: health/prompt/sessions, loopback + token) | port | `e2e_mock_gateway::mode_serve_api_lokal_health_prompt_sessions` |
| RPC extension UI sub-protocol | port | `rpc_mode::tests::{peta_ui_extension_memetakan_event_sesi, frame_ui_extension_roundtrip_decoder}` |
| MCP auth header (env:) + retry 502-504 | port | `tui_mcp::tests::{resolve_header_meneruskan_literal_dan_env_tidak_ada, connect_http_with_mengirim_header_auth_dan_retry_503}` |
| MCP legacy SSE (2024-11-05) + config url/transport | port | `tui_mcp::tests::sse_legacy_handshake_list_dan_call` |
| Spool backpressure (frame tersisa → file) | port | `rpc_mode::tests::{spool_menampung_frame_saat_kanal_penuh, spool_buf_mencegah_keracunan_kunci}` |
| RPC get_state/open_session/antrean prompt | port | `rpc_mode::tests::{state_frame_mencerminkan_sesi, buka_sesi_rpc_memuat_pesannya, prompt_ke_antrean_ditolak_saat_penuh}` |
| Subagent tahap C (panel TUI F3) | port | `tui_mode::tests::panel_f3_menampilkan_daftar_subagent` |
| Subagent tahap B (manager/steer/abort/park/kill/lineage) | port | `manager_tests::{spawn_selesai_dan_hasil_terbaca, kill_mengubah_status_dan_hasil_error, lineage_file_anak_memuat_parent_session, abort_turn_berjalan_menghasilkan_status_aborted}` |
| Notifikasi MCP (progress/log) → langganan | port | `tui_mcp::tests::notifikasi_progress_diteruskan_ke_langganan` |
| Cancel frames host tool/URI (abort/shutdown) | port | `rpc_mode::tests::batalkan_host_tools_mengirim_cancel_dan_melepas_penunggu`, `batalkan_host_uri_mengirim_cancel_dan_melepas_penunggu` |
| MCP E2E lewat turn agent penuh (config → boot → tool call → jawaban) | port | `e2e_mock_gateway::mode_teks_tool_mcp_lewat_turn_penuh` |
| MCP Streamable HTTP (Mcp-Session-Id, respons JSON/SSE, close) | port | `tui-mcp::tests::http_handshake_list_dan_call` (mock HttpListener PowerShell); `docs/mcp-v1.md` |
| RPC host URI schemes (set_host_uri_schemes + host_uri_request/result + wiring tool) | port | `rpc_mode::tests::set_host_uri_schemes_validasi_dan_ganti_set`; `tools::tests::read_dan_write_url_skema_host_dipantulkan_ke_bridge` | `rpc_mode::tests::set_host_uri_schemes_validasi_dan_ganti_set`, `host_uri_result_tanpa_permintaan_ditolak`, `frame_host_uri_roundtrip_decoder` |
| RPC host tools (`set_host_tools`/`host_tool_call`/`host_tool_result`) | port | `rpc_mode::tests::set_host_tools_diregistrasi_dengan_prefix_host`, `set_host_tools_duplikat_ditolak`, `set_host_tools_kosong_menghapus_semua_host`; `tools::tests::replace_named_mengganti_dan_mendaftar` |
| RPC v1.1: `prompt_result` + `session_settled` | port | `rpc_mode::tests::hasil_prompt_memuat_prompt_result_dan_session_settled`, `prompt_result_error_membawa_pesan` |
| MCP klien stdio (handshake, tools/list, tools/call, bridge tool) | port (belum wired ke app) | `tui-mcp::tests::handshake_list_dan_call_tool`, `adapter_tool_berprefix_dan_bertier_exec`; `docs/mcp-v1.md` |
| RPC v2: negosiasi protokol + `rpc_chunk` (reassembly 64 MiB) | port | `rpc::tests::chunk_v2_bolak_balik_dengan_decoder`, `decoder_menolak_urutan_salah`, `negosiasi_protokol_dibaca`; `rpc_mode::tests::negosiasi_v2_membuat_frame_besar_dipecah`; `docs/rpc-v2.md` |
| RPC `model` divalidasi terhadap discovery | port | `rpc_mode::tests::model_rpc_divalidasi_terhadap_discovery` |
| RPC `question`/`answer` (tool `ask`) | port | `rpc::tests::answer_dan_question`, `rpc_mode::tests::handle_ask_mengirim_frame_dan_menyimpan_pending` |
| RPC `approval_request`/`approval` | port | `rpc::tests::approval_request_dan_jawaban`, `rpc_mode::tests::approval_diteruskan_dan_keputusannya_dipetakan` |
| Smoke live gateway `#[ignore]` (C-02) | port | `crates/tui-app/tests/live_gateway.rs` (hanya jalan dengan `--ignored` + kunci) |
| E2E approval mode teks lewat stdin (y) → tool berjalan | port | `tests/e2e_mock_gateway.rs::mode_teks_approval_stdin_mengizinkan_tool` |
| E2E crash di tengah turn → sesi tetap valid | port | `tests/e2e_mock_gateway.rs::crash_di_tengah_turn_tidak_merusak_sesi` |
| E2E mode RPC + approval frame vs gateway tiruan | port | `tests/e2e_mock_gateway.rs::rpc_mengizinkan_tool_lewat_frame_approval` (approval → tool write jalan → result stop) |
| E2E mode teks vs gateway tiruan (tanpa kunci nyata) | port | `tests/e2e_mock_gateway.rs::mode_teks_melawan_gateway_tiruan_dan_merekam_event` |
| Perekam transkrip `--event-log` (C-12) | port | `event_log::tests::event_direkam_dan_diteruskan`, `tanpa_path_tidak_ada_berkas`, `direktori_log_dibuat_otomatis` |
| Runner parity dari transkrip JSONL (C-12) | port | `parity_report::tests::transkrip_identik_dinyatakan_bersih`, `perbedaan_event_dilaporkan_dengan_indeks`, `baris_rusak_dilaporkan_dengan_nomor_baris`, `berkas_tidak_ada_dilaporkan`; smoke CLI exit 0/1 |
| Harness golden baseline (C-12) | port | `agent::tests::golden_teks_murni`, `golden_tool_diizinkan`, `golden_tool_ditolak`, `golden_follow_up`, `golden_length_tanpa_tool`, `golden_dua_tool_dalam_satu_pesan`, `golden_rantai_dua_tool`, `golden_tool_tidak_dikenal`, `golden_error_provider`, `golden_tiga_turn_teks_tool_follow_up`, `golden_abort`, `golden_mendeteksi_mutasi_satu_perilaku`; 11 fixture `tests/golden/*.txt` (baseline sendiri, bukan omp) |
| Normalisasi event + diff parity (komponen C-12) | port | `parity::tests::compare_identik_untuk_urutan_sama`, `compare_menunjuk_event_pertama_yang_berbeda`, `compare_mendeteksi_event_hilang_dan_kelebihan`, `normalisasi_membuang_id_acak` |
| RPC batas frame 1 MiB | port | `frame_besar_ditolak_dan_server_lanjut`, `galat_json_dipotong_pratinjaunya` |
| Timeout approval semua mode | port | `channel_ui_menolak_setelah_timeout`, `channel_ui_meneruskan_keputusan`, `persetujuan_kedaluwarsa_dicatat` |
| Kebijakan config dipakai TUI/teks/RPC | port | `mode_yolo_tidak_bertanya_ke_ui`, `grant_persisten_dilewati_tanpa_bertanya` |
| Status turn §7.2 (`TurnStatus`) | port | `status::tests::transisi_sah_mengikuti_diagram`, `transisi_tidak_sah_ditolak`, `agent::tests::status_turn_selesai_kembali_idle`, `status_turn_dibatalkan_menjadi_aborted_lalu_idle` |
| `/quit` saat sibuk membatalkan turn dulu (TUI & teks) | port | `tui_mode` perilaku quit tertunda; `text_mode::tests::quit_saat_sibuk_memicu_pembatalan` |
| Ctrl+C mode teks: batal saat sibuk, dua kali keluar saat idle | port | `text_mode::tests::ctrl_c_idle_butuh_dua_tekanan`; handler `tokio::signal` di `text_mode::run` |
| Pause gate di batas iterasi/turn (C-03) | port | `agent::tests::pause_di_tengah_turn_berhenti_sebelum_panggilan_model_berikut`, `pause_setelah_tool_menghentikan_turn_sebelum_follow_up`, `pause_menolak_turn_berikutnya` |
| Hook `syncContextBeforeModelCall` (C-03) | port | `agent::tests::context_sync_mengganti_konteks_sebelum_panggilan_model`, `context_sync_kosong_diabaikan` |
| Metadata `AgentTool` label/loadMode/render (C-04) | port | `tool_meta::tests::semua_tool_v1_punya_label_dan_render_tepat`, `tool_asing_mendapat_label_namanya`; `tui_mode::tests::tool_timeline_memakai_label_dan_petunjuk_render` |
| Gate `beforeModelCall` (C-03) | port | `agent::tests::gate_sebelum_panggilan_model_bisa_menghentikan_turn`, `gate_sebelum_panggilan_model_menunggu_bila_tidak_menghalangi` |
| Prompt saat turn sibuk ditolak jelas (C-03) — TUI/RPC/teks | port | `tui_mode::tests::prompt_saat_sibuk_ditolak_dengan_pesan_jelas`, `text_mode::tests::baris_saat_sibuk_ditolak_bukan_diantrekan` |
| Prompt saat turn sibuk ditolak jelas (C-03) | port | `tui_mode::tests::prompt_saat_sibuk_ditolak_dengan_pesan_jelas` |
| Abort saat menunggu approval tidak menggantung | port | `agent::tests::pembatalan_saat_menunggu_approval_tidak_menggantung` |
| Turn tertunda sampai approval diputuskan | port | `agent::tests::turn_tertunda_sampai_approval_diputuskan` |
| TUI: `F2` lompat ke persetujuan | port | `f2_melompat_ke_persetujuan_yang_ter_scroll_jauh` |
| Timeout approval → deny otomatis, tool tidak jalan (C-10) | port | `tui_mode::tests::approval_timeout_menolak_dan_tool_tidak_jalan` (juga memverifikasi penolakan menunggu batas waktu) |
| Dialog persetujuan TUI inline (C-10) | port | `approval_inline_menampilkan_detail_dan_pilihan`, `enter_tanpa_pilihan_tidak_menyetujui`, `tab_dua_kali_memilih_tolak`, `detail_verbatim_menampilkan_argumen_penuh` |
| Approval mode teks (argumen verbatim + batas waktu) | port | `approvals::tests::prompt_mode_teks_memuat_argumen_verbatim_dan_batas_waktu` |
| Tidak ada tool melewati gate approval | port | `agent::tests::tidak_ada_tool_yang_melewati_gate_approval` |
| `/forget-approvals` (hapus grant memori + file) | port | `grants::tests::forget_grants_mengosongkan_memori_dan_file`, `commands::tests::forget_approvals_dikenali` |
| Penjagaan versi file grant | port | `grants::tests::file_grant_versi_baru_ditolak` |
| Grant persisten (`grants.json`) | port | `file_hilang_berarti_kosong`, `simpan_dan_muat_bolak_balik`, `file_rusak_dilaporkan`, `approver_menyimpan_hanya_keputusan_always`, `kegagalan_menyimpan_tidak_menjatuhkan_keputusan` |
| Grant expiry (`approval.grant_ttl_hours`) | port | `grants::tests::grant_kedaluwarsa_diabaikan`, `grant_lama_tanpa_bidang_expiry_tetap_berlaku`, `persisting_menyimpan_waktu_kedaluwarsa_saat_ttl_diisi`, `approvals::tests::grant_kedaluwarsa_tidak_diseed_dan_basi_dipangkas`, `grant_belum_kedaluwarsa_tetap_dipakai`, `config::tests::grant_ttl_default_nol_dan_bisa_diubah` |
| Metode kompaksi bertingkat (`remote`/`snapcompact`/`handoff`) | di luar v1 | RENCANA C-07: metode itu TIDAK di v1; yang ada `soft` + `shake` |
| Mode RPC JSONL (`mx-rpc/1`) | port | `rpc::tests` (6: parse, abort opsional, field tambahan, proyeksi event, tag frame) |
| Harness differential vs omp | ditutup (keputusan) | Paritas omp dilepas (lihat kepala dokumen); harness internal tetap hidup: 11 golden + uji mutasi `golden_mendeteksi_mutasi_satu_perilaku` |
| Probe gateway live (discovery 25 model + turn + tool call + keyring) | port | `V-02-gateway.md` (2026-11-18, smoke `--ignored` hijau 3,61 dtk) |
