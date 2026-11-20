# Changelog

Format mengikuti Keep-a-Changelog. Entri baru selalu di bawah `## [Unreleased]`.

## [Unreleased]

### Changed
- **Keputusan iqbal (2026-11-18): target paritas dengan omp dilepas.** `docs/paritas.md` menjadi ledger
  bukti perilaku internal (141 baris port + uji); F-01 ditutup; C-12 ditutup dengan 11 golden internal,
  uji mutasi, dan rekaman gateway live. omp tetap rujukan sejarah perilaku di komentar/dokumen.
- Keadaan di status line TUI kini diturunkan dari **`TurnStatus` §7.2** (streaming, menjalankan tool, menunggu
  persetujuan, dibatalkan, error) dan status terminal tidak langsung ditimpa `siap` saat `AgentEnd`.
- Registry slash command kini memakai `CommandSpec { name, usage, aliases, description, allow_args }`:
  `/help` dibangun dari registry, alias `?` → `/help` dan `exit` → `/quit`, dan command yang tidak
  menerima argumen menolak argumen dengan pesan jelas (dulu argumen diabaikan diam-diam).
- `Ctrl+C` di TUI tidak lagi langsung menutup: saat turn berjalan ia membatalkan turn lebih dulu,
  dan saat idle perlu dua tekanan berturut-turut (tombol lain membatalkan konfirmasi). `/quit` atau
  `/exit` yang diketik saat turn berjalan kini membatalkan turn dan meminta dijalankan lagi setelah
  idle, sesuai kriteria C-08/C-09.
- Normalisasi parity kini ikut membandingkan **stop reason** (`message_end:ok:stop|tooluse|length|…`),
  supaya skenario `length`/`toolUse` tidak lagi terlihat identik dengan jawaban biasa.
- `maicli config show` menyebut **sumber** kunci API (`env`, `keyring`, atau `tidak ada`) alih-alih
  selalu menulis "environment", tanpa pernah menampilkan nilainya.
- Seluruh penulisan entry sesi (pesan, compaction, approval, usage, judul, model) dipindah ke
  `spawn_blocking` lewat `session_open::append`, sehingga tidak ada lagi fs sinkron di jalur async —
  melengkapi perpindahan pemuatan awal di putaran sebelumnya (§4).
- Pemuatan awal dipindah ke `spawn_blocking`: muat config, resolusi/resume sesi, dan buka/buat
  store sesi di mode teks, TUI, dan RPC tidak lagi menjalankan fs sinkron di dalam future (§4).

### Added
- **Wizard setup awal (first-run)**: tanpa kunci → pilih provider → kunci tersembunyi
  → keyring → discovery model → config tertulis; jalan di Windows/Linux.
- **Sistem GitHub**: workflow CI (matriks Linux+Windows, gerbang penuh) + Release
  otomatis dari tag v* (build 3 platform + catatan dari CHANGELOG) + LICENSE
  MIT + CONTRIBUTING + SECURITY + panduan setup.
- **Packaging Linux x64**: `maicli-0.1.0-linux-x64.tar.gz` (release + strip, 3,8 MB).
- **kill-tree POSIX**: child = leader proses group (`process_group(0)`) + `kill -9 -- -pgid`
  (tanpa unsafe); test pohon 2 level di Linux nyata.
- **Keyring Linux** (Secret Service) + **UI interaktif**: `/model` picker,
  `/provider add` form bertahap (simpan `[providers_eksternal.<id>]` di config global,
  kredensial tetap env-only).
- **Verifikasi Linux native**: `check.sh` SEMUA GERBANG HIJAU di server iqbal
  (x86_64; 501 test + 9 E2E); \cl_restricted\ POSIX kini memeriksa mode 600;
- **ACP transport (subagent D)**: `maicli --mode acp` — agen sebagai server ACP
  (JSON-RPC line-delimited stdio) untuk host eksternal dan lokal; multi-sesi + cancel.
- **Extensions (v1)**: loader extension eksternal — folder + manifest `extension.json`
  ketat; tool proses dengan env dibersihkan + approval Exec + nama `ext.<ext>.<tool>`;
  config `[extensions] enabled` + flag `--no-extensions`.
- **Agent Orchestrator**: tool `orchestrate` memunculkan banyak subagent paralel/berurutan;
  semua anak terlihat di Agent Hub.
- **Integrasi katalog runtime (v1)**: config `[catalog] path`, flag `--catalog <DIR>`,
  command interaktif `/catalog` (picker provider + validasi model bawaan + hint env).
- **Agent Hub v2**: transkrip asisten anak (toggle `[t]`) + broadcast steer ke semua
  anak berjalan (`[b]`).
- **`maicli serve` `GET /api/subagents`**: daftar subagent (status/turns) lewat HTTP API.
- **RPC `replay_spool`** (v2): klien menarik ulang frame yang tertahan di spool.
- **RPC `subagent_list`** (v2): klien memantau daftar subagent (status/turns/tool_calls);
  tool `subagent` kini aktif di mode RPC.
- **Event anak → induk (subagent tahap C)**: `SubagentHandle::events()` — seluruh event
  sesi anak diteruskan dengan retensi tunggakan 200.
- **SSE legacy notifikasi**: pesan tanpa id di stream GET diteruskan; tunggakan
  pra-langganan disimpan (maks 100).
- **MCP HTTP notifikasi long-lived + reconnect**: GET stream SSE dengan sambung ulang
  otomatis (EOF → 500 ms); server yang menolak stream dibiarkan diam.
- **RPC `login` → `login_status`** (v2): cek kesiapan kunci env provider tanpa
  membocorkan nilainya.
- **RPC `set_config` → `config_update`** (v2): klien mengganti `models.default` runtime
  (validasi discovery); kunci lain ditolak bernama.
- **Hook `before_tool`** (SDK/`AgentSession`): observasi (nama, preview argumen) sebelum
  eksekusi tool, setelah approval lolos.
- **KDL tahap 5 — `cari_axis`**: resolusi cascade → axis untuk (provider, model) dengan
  pencocokan glob family dan spesifisitas menimpa.
- **KDL tahap 3**: stratum **runtime/behavior** — heuristik OpenAI-Responses + evaluasi,
  provider pensiun/isolasi/append-only (oracle omp nyata).
- **KDL tahap 2**: strata **classes + taxonomy** ter-parse (40 aturan; namespace/family/override/
  revision; oracle omp nyata). Sisa: runtime/cascade.
- **SDK `AgentRegistry`**: sesi paralel dalam satu proses — daftarkan builder per id,
  bangun (ulang), daftar, hapus.
- **RPC `command` → frame `command_output`** (v2): klien menjalankan perintah slash read-only
  (usage/context/todo/tree/model) lewat protokol.
- **RPC `set_title` + frame `session_info_update`** (v2): klien mengganti judul sesi dan
  menerima info sesi (id/model/judul) setelah `set_title`, `model`, dan `open_session`.
- **Hook internal `after_tool`** (`AgentSession::set_after_tool`): observasi (nama tool,
  is_error, cuplikan hasil) di semua jalur eksekusi — sukses, gagal, ditolak approval, paralel.
- **Notifikasi MCP di timeline**: `[mcp:<server>]` diteruskan ke TUI/teks; task penerus
  berhenti sendiri (idle-timeout 3×2 dtk) + abort saat keluar — anti-deadlock.
- **SSE push `GET /api/events/stream`**: tamu menerima event riwayat realtime (Last-Event-ID,
  koneksi ditahan sampai putus).
- **Token serve otomatis**: `MAICLI_SERVE_TOKEN` env atau token acak di `<data>/serve-token` (0600/ACL)
  — server HTTP lokal tidak pernah berjalan tanpa otentikasi.
- **Session tree**: `tui_session::pohon` (relasi fork dari header) + perintah **`/tree`**
  di mode teks dan TUI (indentasi per kedalaman cabang).
- **Skills v1**: folder `skills/<nama>/SKILL.md` (+`MAICLI_SKILLS_DIR`) — daftar disuntik ke system prompt,
  tool `skill` membaca isi (tier Read).
- **Serve stateful**: transkrip+usage tersimpan antar request; `GET /api/session` (metadata),
  `POST /api/abort` (cancel token turn aktif); prompt saat sibuk → 409.
- **RPC `abort_and_restore_queue` (v2)**: abort turn berjalan dan seluruh antrean dikembalikan klien
  lewat frame `queue_restored` (antrean dikosongkan).
- **RPC antrean lanjutan (v2)**: `remove_queued_message` dan `promote_queued_message`
  (posisi antrean saat turn berjalan; notice + error posisi tidak ada).
- **secure_file POSIX**: file sesi/config/grant di Linux/macOS kini chmod 0600 (test unix; sebelumnya no-op).
- **`tui-katalog`** (F-06 tahap 1): kompilator KDL katalog omp — strata providers (98 aturan dari
  105 berkas) + auth (108) ter-parse dan terverifikasi terhadap katalog nyata.
- **Collab v1 (polling)**: `GET /api/events` — tamu memantau riwayat event sesi server
  (prompt/jawab/turn/tool/notice, 1000 terakhir). WebSocket relay penuh menyusul (butuh dependensi WS).
- **API server lokal**: `maicli --mode serve --port` — HTTP/1.1 minimal di loopback (tanpa dependensi baru):
  `GET /api/health`, `POST /api/prompt` (satu turn penuh), `GET /api/sessions`; auth opsional Bearer
  via `MAICLI_SERVE_TOKEN`; tool tier manusia ditolak otomatis di server tanpa UI.
- **RPC extension UI sub-protocol (v2)**: `set_ui_extension` — klien melanggani event sesi
  (`turn.start`/`turn.end`/`tool.start`/`tool.end`) dan menerima frame `ui_extension`.
- **MCP auth header + retry**: `mcp.servers.x.headers` (nilai `env:NAMA`; tidak pernah dicatat log),
  dan retry satu kali untuk 502/503/504 pada transport HTTP.
- **MCP legacy SSE (2024-11-05)**: `connect_sse` (GET event-stream → `endpoint` → POST; balasan
  di-stream GET, korelasi per id) + config `mcp.servers.x.url` dan `transport = "http"|"sse"`.
- **Spool backpressure RPC**: kanal keluar bounded 4096; frame yang tidak terkirim (kanal penuh/
  stdout putus) ditulis ke `rpc-spool-<pid>.jsonl` di direktori data (batas 64 MiB) — tidak ada
  frame hilang diam-diam.
- **RPC sesi/antrean**: `get_state` (frame `state`: model, pesan, busy, antrean, usage), `open_session`
  (id/prefix/terbaru; mengganti sesi aktif), dan **antrean prompt saat turn berjalan** (maksimum 8,
  dijalankan berurutan setelah turn selesai; penuh → `E_AGENT_BUSY`).
- **Subagent tahap C**: panel **F3** di TUI (mini Agent Hub) — daftar subagent dengan status,
  jumlah turn/tool, dan potongan hasil; tool `subagent` terhubung ke panel lewat `with_manager`.
- **Subagent tahap B**: `SubagentManager`/`SubagentHandle` — kontrol anak live (steer/abort/park/resume/kill),
  enam status siklus hidup, dan lineage sesi anak (`parent_session`) via `SessionStore::create_with_parent`.
- E2E **tool MCP lewat turn agent penuh** (`mode_teks_tool_mcp_lewat_turn_penuh`): server MCP PowerShell
  sungguhan via config proyek `[mcp.servers.e2e]`, model memanggil tool MCP, hasilnya kembali di jawaban.
- **Registry provider** (`tui-wire::providers`): **ModeluXAI** (bawaan, `https://api.modeluxai.com/v1`,
  kunci `MODELUXAI_API_KEY`) + 45 provider katalog omp (OpenAI, OpenRouter, DeepSeek, zAI/GLM, Groq,
  Moonshot, xAI, Mistral, Together, Fireworks, Cerebras, Gemini, Ollama/LM Studio/vLLM lokal, dll.).
  Pengguna cukup memasukkan **API key** (env var atau keyring lewat `maicli config key-import`). Command
  baru **`/provider`**: daftar provider dan ganti provider aktif (menulis config global; restart
  diperlukan sesuai kebijakan v1).
- **Subagent lokal tahap A (`tui-subagent`)**: tool `subagent` yang menjalankan sesi anak terisolasi
  (tool + approval + deadline sendiri) dan mengembalikan jawaban akhirnya ke induk; bertier `exec`,
  gagal anak tidak menjatuhkan induk, tidak bersarang. 5 test; dokumen `docs/subagent-v1.md`.
- **MCP Streamable HTTP**: `connect_http` dengan pelacakan `Mcp-Session-Id`, respons JSON atau SSE
  (korelasi id, `[DONE]` dilewati), dan `close()` (DELETE sesi); backend koneksi stdio/HTTP bersatu
  di `McpConnection`. **Peta P1 integrasi dasar selesai** (`docs/platform-dan-integrasi.md`).
- **Host URI schemes RPC (v2)**: `set_host_uri_schemes` (validasi nama skema, ganti set penuh) dan
  sub-protokol `host_uri_request`/`host_uri_result` untuk baca/tulis "berkas virtual" milik host;
  wiring ke tool file agent kini terpasang: `read`/`write` memantulkan URL skema host lewat
  `ToolContext.uri_bridge` (`UriResolver`) ke klien RPC.
- **Host tools RPC (v2)**: host mendaftarkan tool sendiri lewat `set_host_tools`; permintaan eksekusi
  model diteruskan sebagai `host_tool_call` dan dijawab `host_tool_result`; tool host bertier `exec`
  (approval tetap berlaku), timeout 120 detik. `ToolRegistry` dapat `replace_named`/`remove_named`.
- **RPC v1.1**: frame `prompt_result` (`status` ok/aborted/error, `agent_invoked`) dikirim setelah
  `result` dengan `id` sama, lalu `session_settled` menandai sesi siap prompt berikutnya.
- **`tui-mcp` (MCP stdio)**: klien handshake `initialize`/`tools/list`/`tools/call`, bridge tool
  `mcp.<server>.<tool>` bertier `exec`, timeout per request, env kredensial tidak diwariskan; 4 test dengan
  mock server MCP PowerShell. Terhubung lewat config `[mcp]`/`[mcp.servers.*]` (validasi ketat), boot `mcp_boot::connect_all`
  (server gagal tidak menjatuhkan aplikasi), registry semua mode, catatan status, dan label timeline
  `mcp`. Dokumen `docs/mcp-v1.md` + bagian `[mcp]` di `docs/config-v1.md`.
- **RPC v2 transport**: negosiasi versi (`ready.supported_protocol_versions`, `negotiate_protocol`,
  frame `negotiated`) dan chunking frame besar (`rpc_chunk` base64, reassembly sampai 64 MiB) dengan
  encoder/decoder tervalidasi; v1 tetap default dan kompatibel. Dokumen `docs/rpc-v2.md`.
- **SDK Rust v1 (`tui-sdk`)**: `SessionBuilder`/`Session`/`EventStream` untuk embedding agen
  tanpa TUI — provider/approver wajib, tool set bisa diganti, event dua tingkat, abort/pause/model.
  5 test + 1 doc-test; dokumen `docs/sdk-rust-v1.md`; status P1 di `docs/platform-dan-integrasi.md`.
- Dokumentasi `docs/platform-dan-integrasi.md`: matriks dukungan Windows/Linux/macOS native, SDK, API,
  RPC, WebSocket, MCP, dan agent-to-agent; analisis kesenjangan terhadap omp 18.22.0; peta jalan
  pembangunan P1–P3 beserta kriteria penerimaan dan keputusan scope yang diperlukan.
- Korpus golden baseline bertambah: `tiga-turn` (teks → tool_call → follow-up, 3 panggilan model) dan
  `abort` (pembatalan di tengah stream), total **11 fixture** (baseline sendiri, bukan omp).
- Status line TUI kini menampilkan **mode approval** (`tanya`/`tanya+cache`/`tulis`/`yolo`) sesuai C-09,
  di samping model, persen konteks, keadaan, dan status.
- Mode teks kini setara TUI untuk `/quit` saat turn berjalan: perintah itu **membatalkan turn** lalu meminta
  dijalankan lagi setelah idle, bukan ditolak sebagai “masih sibuk”.
- Mode teks kini menangani **Ctrl+C**: saat turn berjalan membatalkan turn (seperti TUI), saat idle tekanan
  pertama meminta konfirmasi dan tekanan kedua keluar. Fitur `tokio` `signal` ditambahkan.
- Konsistensi mode teks: baris yang diketik **saat turn berjalan** kini ditolak dengan pesan “masih sibuk;
  kirim ulang…” alih-alih diantrekan diam-diam sebagai prompt berikutnya (setara TUI/RPC); jawaban untuk
  tool `ask` tetap diproses.
- RPC `model` kini **divalidasi terhadap daftar discovery** seperti `/model` di TUI/teks: id tak dikenal ditolak
  `E_MODEL_UNKNOWN` tanpa mengubah model; daftar kosong (discovery gagal) meneruskan id apa adanya.
- Test `pause_setelah_tool_menghentikan_turn_sebelum_follow_up`: jeda yang dipicu dari dalam tool menghentikan
  turn sebelum follow-up diproses (sesuai “pause gate di batas turn” RENCANA C-03); memutus stream yang
  sedang berjalan tetap tugas `cancel`.
- **Penjagaan versi skema sesi**: file dengan `version` lebih baru dari yang didukung (atau versi 0) ditolak
  dengan pesan yang menyebut versi file dan versi implementasi; kebijakan kompatibilitas didokumentasikan
  di `docs/skema-sesi-v1.md`. Migrasi belum diperlukan karena tidak ada format sesi lama di v1.
- **Grant expiry** (deviasi kecil dari C-05, dicatat): `approval.grant_ttl_hours` (0 = tidak kedaluwarsa);
  grant `always` menyimpan `expires_at_ms`, grant basi tidak di-seed dan dipangkas saat start, file grant
  lama tanpa field itu tetap kompatibel.
- Smoke **live gateway** `#[ignore]` (`crates/tui-app/tests/live_gateway.rs`): discovery `/v1/models` lebih dulu
  (model tidak ditebak), lalu satu turn streaming; hanya jalan dengan `--ignored` dan kunci nyata.
- Hook **syncContextBeforeModelCall** (C-03): AgentSession::set_context_sync menerima pengganti konteks
  sebelum compaction/panggilan model (hasil kosong diabaikan).
- Metadata **`AgentTool`** (C-04): `label`, `loadMode`, `render` dipisah dari definisi wire; TUI memakai label
  dan petunjuk render (❯ perintah, ± diff, → biasa) di timeline, termasuk tool tak dikenal.
- `check.sh`: padanan POSIX `check.ps1` (fmt/clippy/test/deny + aturan dependensi), memenuhi C-01;
  belum dieksekusi di mesin ini karena Windows tanpa bash/WSL — dicatat apa adanya di `WORK.md`.
- Test **E2E redaksi kunci**: binary dijalankan dengan `MODELUXAI_API_KEY` palsu, lalu seluruh berkas di
  `MAICLI_DATA_DIR` (sesi, log, cache) dipindai — tidak ada yang memuat kunci (C-06 kriteria 5).
- Test **E2E approval mode teks lewat stdin**: tool `write` diminta model, prompt `[persetujuan]` dengan
  **argumen verbatim** tampil, jawaban `y` di stdin membuat tool berjalan dan berkas hasil tertulis.
- Override direktori data **`MAICLI_DATA_DIR`** (portable/test): memindahkan config, sesi, log, dan cache
  tanpa menyentuh profil pengguna; dipakai test E2E agar benar-benar terisolasi.
- Test **E2E crash di tengah turn**: proses `maicli` dibunuh saat streaming menggantung; berkas sesi yang
  sudah ditulis tetap bisa dibaca (`SessionStore::load`), header utuh, pesan pengguna bertahan.
- `--event-log` kini **di-flush sebelum proses keluar** (`event_log::flush`), sehingga transkrip lengkap
  dan deterministik.
- Test **E2E mode RPC + approval**: binary `maicli --mode rpc` melawan gateway tiruan, model meminta tool
  `write`, klien membalas frame `approval_request` dengan `once` → tool berjalan, berkas hasil berisi
  data mock, `result.stop_reason=stop`, lalu `shutdown` menutup server dengan exit 0.
- Test **E2E tanpa gateway nyata** (`crates/tui-app/tests/e2e_mock_gateway.rs`): binary `maicli` dijalankan
  mode teks melawan server HTTP tiruan (SSE + `/models`) dengan kunci palsu dan `MAICLI_DATA_DIR` terisolasi;
  memverifikasi stdout jawaban, exit 0, dan transkrip `--event-log` berisi `AgentStart`/`AgentEnd`.
- Perekam transkrip `--event-log <FILE>` (RENCANA C-12): setiap `AgentEvent` yang mengalir ke UI juga
  ditulis sebagai JSONL (direktori dibuat otomatis, kegagalan menulis tidak menggagalkan sesi), di mode
  TUI/teks/RPC; dipasangkan dengan runner `maicli --parity-expected/--parity-actual`.
- Runner parity (RENCANA C-12): `maicli --parity-expected <ACUAN.jsonl> --parity-actual <HASIL.jsonl>` membaca
  dua transkrip event agen, menormalisasi keduanya, mencetak laporan diff (skenario, indeks event pertama
  berbeda, event hilang/kelebihan), dan exit 1 bila berbeda. `AgentEvent`/`ProviderEvent` kini
  serializable agar transkrip bisa disimpan dan dibandingkan.
- Bukti langsung **kill-tree `bash`**: test `timeout_membunuh_anak_proses` menjalankan anak proses yang  menulis marker setelah 2,5 detik dengan timeout tool 1 detik; marker tidak pernah terbentuk, jadi
  anak proses benar-benar ikut dibunuh (`taskkill /T`), bukan hanya shell induk.
- `docs/tools-v1.md`: skema JSON + contoh panggilan untuk kedelapan tool v1 beserta tier,
  concurrency, batas output, kasus gagal, dan **shell yang dipakai `bash`** (`cmd.exe /C` di Windows).
- Test batas keamanan tool: path traversal gaya Windows (`..\`), path absolut di luar direktori kerja,
  dan semantik `edit` yang mencocokkan teks **persis** termasuk akhir baris (CRLF).
- Mode approval **`yolo`** kini selalu menampilkan banner peringatan saat start (TUI sebagai baris pertama,
  mode teks/`--prompt` di stdout, RPC sebagai event `notice`), menyebut bahwa tool berjalan tanpa
  persetujuan dan cara mengubahnya; default tetap `AlwaysAsk` dan tidak pernah ditulis otomatis
  (RENCANA §10 butir 11, `tui_app::banner`).
- File **config, grant approval, dan sesi** kini dibatasi ACL-nya ke pengguna saat ini saja
  (`icacls /inheritance:r /grant:r <user>:F`), termasuk berkas rotasi sesi; diterapkan saat menulis
  dan diverifikasi di test lewat `Get-Acl` (1 ACE, warisan mati, pengguna saat ini) — RENCANA §10 butir 9
  dibuktikan, bukan diasumsikan.
- Cache daftar model di **berkas** `models-cache.json` (RENCANA C-02): TTL 24 jam, tulisan atomik
  (tmp + rename), dipakai saat start agar `/model` punya daftar tanpa memanggil jaringan; file rusak,
  kosong, atau kedaluwarsa diabaikan dan diambil ulang dari gateway. Dependensi `serde` ditambahkan
  ke `tui-app` untuk (de)serialisasi cache.
- Test integrasi **timeout approval** (C-10): approval yang dibiarkan tanpa jawaban ditolak otomatis
  setelah `limits.approval_timeout_secs`; tool tidak pernah mulai (`ToolExecutionStart` tidak ada),
  penolakan tercatat di timeline, dan test memastikan penolakan benar-benar menunggu batas waktu.
- Perintah garis miring kini tampil dengan badge **`[perintah]`** di timeline TUI (prompt biasa tetap
  `>`), termasuk perintah yang dikirim dari pemilih sesi — jelas mana yang dijalankan aplikasi dan mana
  yang dikirim ke model (RENCANA C-08).
- Status turn eksplisit (RENCANA §7.2): `tui_core::status::TurnStatus`
  (`Idle`/`Streaming`/`ExecutingTools`/`AwaitingApproval`/`Aborted`/`Error`) dengan tabel transisi yang
  diuji, dan `AgentSession::status()` yang diperbarui di titik transisi (dijaga `debug_assert`).
- Gate **`beforeModelCall`** (RENCANA C-03): `AgentSession::set_before_model_call(callback)` dipanggil
  sebelum setiap permintaan model; callback mengembalikan `Some(alasan)` untuk menghentikan turn
  (`StopReason::Aborted`) **tanpa** memanggil provider. Diuji dua arah (menghentikan dan meloloskan).
- **Grant expiry** (deviasi kecil dari C-05, dicatat): `approval.grant_ttl_hours` (0 = tidak kedaluwarsa);
  grant `always` menyimpan `expires_at_ms`, grant basi tidak di-seed dan dipangkas saat start, file grant
  lama tanpa field itu tetap kompatibel.
- Smoke **live gateway** `#[ignore]` (`crates/tui-app/tests/live_gateway.rs`): discovery `/v1/models` lebih dulu
  (model tidak ditebak), lalu satu turn streaming; hanya jalan dengan `--ignored` dan kunci nyata.
- Hook **syncContextBeforeModelCall** (C-03): AgentSession::set_context_sync menerima pengganti konteks
  sebelum compaction/panggilan model (hasil kosong diabaikan).
- Metadata **`AgentTool`** (C-04): `label`, `loadMode`, `render` dipisah dari definisi wire; TUI memakai label
  dan petunjuk render (❯ perintah, ± diff, → biasa) di timeline, termasuk tool tak dikenal.
- Harness golden baseline bertambah menjadi **6 skenario** (teks murni, tool diizinkan, tool ditolak,
  follow-up 2 turn, `length`, dua tool sejajar) — semuanya baseline implementasi sendiri.
- Harness golden baseline (C-12): fixture `tests/golden/*.txt` untuk skenario teks murni, tool
  diizinkan, dan tool ditolak, dibandingkan dengan transkrip ternormalisasi melalui `parity::compare`;
  test mutasi membuktikan harness menjadi MERAH saat satu event diubah (laporan menyebut indeks).
  Fixture ini baseline implementasi sendiri, **bukan** transcript omp.
- Komponen pembanding parity (`tui_wire::parity`, bagian C-12): normalisasi event (membuang id acak,
  timestamp, dan event UI murni) serta `ParityDiff` yang menyebut skenario, indeks event pertama yang
  berbeda, dan daftar event hilang/kelebihan — laporan diff, bukan sekadar “gagal”.
- Test integrasi worker: prompt yang dikirim **saat turn sibuk** ditolak dengan notice “masih sibuk”
  (server SSE lambat tiruan), membuktikan C-03(4) di tingkat produk, bukan hanya core.
- Hapus sesi (`dropSession`, RENCANA C-06): `maicli --drop-session <id>` menghapus berkas sesi dan
  artefak lock-nya; menolak sesi yang sedang dipakai proses lain (`E_SESSION_LOCKED`) dan melaporkan
  sesi tak ada (`E_SESSION_NOT_FOUND`). Operasi destruktif hanya lewat flag eksplisit, tidak otomatis.
- Pencarian judul sesi (RENCANA C-06): `tui_session::search` (substring, tanpa peduli huruf besar),
  `/sessions <judul>` di aplikasi, dan `maicli --list-sessions [judul]` di CLI.
- Approval di mode teks kini menampilkan **argumen verbatim** (tidak hanya subjek) dan batas waktu
  keputusan, sehingga pengguna tidak menyetujui tool tanpa melihat apa yang akan dijalankan.
- `docs/uat-v1.md`: paket UAT U-01..U-12 (langkah, hasil yang diharapkan, kolom hasil untuk
  iqbal) sesuai RENCANA §11.3, termasuk prosedur log untuk bukti performa.
- Instrumentasi pengukuran C-09: dengan MAICLI_LOG=debug, TUI mencatat frame_ms per frame dan
  delta_ms (dari delta streaming diterima sampai digambar) ke berkas log; `logging::init_with`
  ditambahkan agar jalur ini bisa diuji (log benar-benar tertulis ke berkas).
- Test regresi C-05: seluruh 8 tool v1 dicoba dengan approver Deny — setiap panggilan berhenti di
  gerbang (placeholder error, tanpa efek samping), membuktikan tidak ada jalur eksekusi yang melewati gate.
- Berkas sesi punya **batas ukuran 16 MiB + rotasi**: saat batas tercapai, sesi lanjut di berkas baru
  dengan `parent_session` menunjuk berkas sebelumnya (berkas lama utuh), sehingga log tidak tumbuh
  tanpa batas (RENCANA C-06).
- Override config **runtime** lewat `maicli --set bagian.kunci=nilai` (boleh diulang): prioritas tertinggi,
  tidak menulis file, nilai divalidasi dulu, dan ditandai `<runtime --set>` di `config show`.
- `docs/config-v1.md`: skema config v1 (semua bagian/kunci/default), urutan pencarian kunci API,
  dan catatan bahwa perubahan config **butuh restart** (tanpa file-watching) sesuai C-11.
- Test regresi: `finish_reason: content_filter` dipetakan ke `stopReason` error dengan pesan yang menyebut
  `content_filter` (bukan sukses diam-diam), sesuai kriteria C-02.
- Test integrasi membuktikan turn **benar-benar tertunda** saat approval menunggu: tool belum
  dieksekusi sebelum keputusan, lalu berjalan setelah izin; blok approval diuji menempel tepat
  setelah baris histori terakhir.
- `F2` melompat ke blok persetujuan yang menunggu saat histori sudah ter-scroll jauh; hint
  tombol pada status ikut menyebutkannya.
- Dialog approval di TUI kini **inline di timeline** (overlay modal dihapus): detail **verbatim**
  (subjek + argumen utuh), pilihan `izinkan sekali`/`tolak` lewat **Tab** lalu **Enter** (pintas
  `y/2/3/n` tetap ada), dan **countdown** dari `limits.approval_timeout_secs`. Enter tanpa pilihan
  tidak menyetujui, dan keputusan/kedaluwarsa berlabel teks pada blok yang sama.
- Tool dijalankan sesuai konkurensinya: beberapa tool `shared` dalam satu respons dijalankan
  **bersamaan** (spawn), sedangkan tool `exclusive` selalu sendirian; `ToolDefinition` kini membawa
  field `concurrency` (default `shared`). Bukti: dua test dengan jam tokio dijeda.
- `/forget-approvals` menghapus semua grant yang diingat dari memori **dan** mengosongkan
  file `grants.json` (dulu grant hanya bisa bertambah); approver kini mengekspos `forget_grants`.
- Tool `todo` diturunkan ke tier `read` sesuai RENCANA C-04 (mengubah daftar kerja bukan
  menyentuh berkas), sehingga tidak memicu approval tulis.
- Timeline tool di TUI bisa **dibuka/diringkas dengan `Ctrl+E`**: detail disimpan per baris tool
  (mulai/selesai + cuplikan), dan perhitungan gulir ikut menghitung baris detail yang tampil.
- TUI punya **palet command Ctrl+K**: filter berdasarkan nama/usage/deskripsi, `↑`/`↓` memilih,
  Enter menjalankan command tanpa argumen atau mengisi composer untuk command yang butuh argumen,
  `Esc` menutup tanpa efek.
- Composer TUI kini **multi-baris**: `Shift+Enter`/`Alt+Enter` menambah baris baru (Enter tetap
  mengirim), tinggi kotak mengikuti isi sampai 12 baris dan menggulir internal agar baris terakhir
  terlihat; petunjuk tombol ikut diperbarui.
- `/model` memakai hasil discovery gateway: tanpa arg menampilkan model aktif **dan** daftar model
  yang tersedia (atau menyebut bahwa daftar belum diambil), argumen tak dikenal ditolak dengan daftar
  pilihan alih-alih dipakai menebak; daftar diambil sekali saat start dengan batas 3 detik dan tidak
  menggagalkan aplikasi bila gateway belum bisa dihubungi.
- Command tak dikenal kini menyebut semua command yang tersedia, bukan hanya menyuruh `/help`.
- `/new` memulai sesi baru (konteks dan usage direset) dan `/fork` menduplikasi sesi berjalan lalu
  melanjutkan salinannya — keduanya tersedia di mode teks dan TUI; di TUI histori layar dikosongkan
  lewat event `SessionSwitched` yang sama dengan `/switch`.
- C-07 `fitOutputTokens`: `max_tokens` per panggilan kini menyesuaikan sisa window konteks dengan
  lantai 1.024 token; bila `max_tokens` tidak dikonfigurasi, nilainya tetap kosong (tidak mengarang).
- `/context` menampilkan pemakaian konteks terhadap anggaran (persen, reserve, target pemulihan) dan
  `/todo` menampilkan daftar todo sesi dari tool `todo` (daftar kini dibagikan lewat `ToolRegistry`).
- Metode compaction **`shake`** sesuai RENCANA C-07: saat konteks melewati ambang, isi hasil tool
  lama dipangkas lebih dulu tanpa memanggil model; bila hasilnya kembali di bawah target pemulihan,
  ringkasan model tidak diperlukan. Minimal 8 hasil tool terbaru selalu dipertahankan, dan bila
  `shake` tidak cukup konteks diringkas dengan metode `soft` seperti sebelumnya.
- Composer TUI punya **riwayat prompt**: saat kosong, `↑` memanggil prompt sebelumnya dan `↓` yang
  lebih baru (melewati entri terbaru mengosongkan composer); saat berisi teks, panah tetap menggulir
  histori seperti sebelumnya.
- Dialog persetujuan tool di TUI sekarang punya overlay sendiri: nama tool + tier, subjek, dan
  **cuplikan argumen** (dipotong agar tidak memenuhi layar), sehingga yang diizinkan terlihat jelas.
- `/compact` melaporkan hasilnya: ukuran konteks sebelum dan sesudah (perkiraan token) serta jumlah
  pesan yang digantikan ringkasan; `compact_now` mengembalikan `CompactionReport` alih-alih `bool`.
- TUI menerima **tempel teks** (bracketed paste): baris baru diubah menjadi spasi, isi dibatasi
  4000 karakter seperti ketikan, dan `Ctrl+U` membersihkan composer. Fitur `bracketed-paste`
  crossterm diaktifkan beserta pasangan disable-nya di `TermGuard`.
- Tabel markdown di histori TUI dirender dengan lebar kolom seragam dan header tebal beraksen;
  hanya baris dengan pemisah `|---|` yang dianggap tabel, sehingga teks berpipa biasa tidak berubah.
- Blok kode berpagar (```) di histori TUI tampil redup dan **tidak** diproses sebagai penekanan,
  termasuk baris yang sedang streaming di dalamnya, sehingga kode berisi `**` tidak berubah gaya.
- Histori TUI memakai **penekanan ringan**: heading `# …` tebal berwarna aksen, `**tebal**` dan
  `` `kode` `` diberi gaya; penanda yang tidak ditutup dibiarkan sebagai teks biasa agar keluaran
  setengah jadi tidak hilang.
- Status bar TUI menampilkan **pemakaian konteks** (`konteks N%`) dari perkiraan token sesi dan
  `limits.context_window_tokens`; window 0 tidak ditampilkan (tidak ada pembagian nol). Event baru
  `ContextUsage` membawa angka itu ke UI.
- TUI punya **pemilih sesi**: `/sessions` membuka overlay berisi sesi terbaru (judul + jumlah
  pesan), `↑`/`↓`/`j`/`k` memilih, `Enter` langsung pindah sesi, `Esc` menutup tanpa efek; daftar
  kosong tetap menjadi pesan biasa. Event baru `SessionsListed` membawa daftar itu ke UI.
- `maicli --fork [ID]` membuat salinan sesi (judul "… (fork)", `parent_session` terisi) lalu melanjutkan
  salinannya; `--resume` dan `--fork` tidak boleh dipakai bersamaan.
- `/usage` merinci pemakaian **per model** bila sesi memakai lebih dari satu model (termasuk setelah
  `/model` berganti), dihitung dari akumulasi per model di `AgentSession`.
- Kunci API bisa dibaca dari keyring OS: env var tetap prioritas, keyring jadi cadangan, dan
  `maicli config key-import` menyimpan nilai dari env var ke Windows Credential Manager tanpa pernah
  mencetaknya (pesan galat pun tidak memuat nilai kunci).
- Entry `ModelUsage` kini ditulis setiap turn (selisih terhadap yang sudah tercatat) dan dijumlahkan
  saat resume, sehingga `/usage` melanjutkan hitungan sesi alih-alih mulai dari nol; sebelum ini
  entry tersebut hanya ada di skema.
- Entry `ModelChange` kini ditulis saat `/model` dan dibaca saat resume; model sesi bertahan antar
  proses dengan precedensi `--model` > model terakhir di sesi > `models.default` (sebelumnya entry
  ini hanya ada di skema dan tidak pernah dipakai).
- Entry `TitleChange` kini benar-benar dipakai: judul terakhir menang atas judul header, sehingga
  `/title` dan `--list-sessions` menampilkan nama yang benar (sebelumnya entry ditulis tapi diabaikan).
- `/title <teks>` mengganti nama sesi di mode teks dan TUI (ditulis sebagai entry `TitleChange`).
- TUI bisa pindah sesi dengan `/switch [id]`: worker membangun ulang sesi, membuka store lewat
  `spawn_blocking`, dan event baru `SessionSwitched` mengosongkan histori layar supaya pesan sesi
  lama tidak tercampur dengan sesi baru.
- `/switch [id]` di mode teks: pindah sesi tanpa keluar aplikasi (`latest`, id persis, atau awalan
  id yang unik; awalan ambigu ditolak dengan daftar kandidat), membuka store baru lewat
  `spawn_blocking`. Di TUI perintah ini menjawab terus terang bahwa pindah sesi belum didukung.
- TUI mendukung roda mouse: satu notch menggulir 3 baris, capture diaktifkan bersama raw mode dan
  dimatikan lagi saat keluar sehingga terminal tetap bersih.
- RPC membatasi satu frame pada 1 MiB: frame lebih besar dibuang tanpa disimpan ke memori, server
  membalas `E_PROTOCOL` dan melanjutkan; galat JSON hanya memuat pratinjau 200 karakter.
- `maicli config set <key> <value>` menulis satu nilai ke config global secara atomik, mempertahankan
  key lain, dan menolak key asing atau nilai tidak valid sebelum file disentuh.
- Slash `/sessions` menampilkan sesi tersimpan (terbaru lebih dulu) di mode teks dan TUI, memakai
  daftar yang sama dengan `maicli --list-sessions`.
- Mode teks sekarang bisa menjawab tool `ask`: stdin dibaca satu thread pembaca dan dibagi antara
  REPL dan pertanyaan; baris yang diketik saat turn berjalan disimpan untuk prompt berikutnya,
  bukan dibuang.
- Gulir TUI menghitung baris **visual** hasil word-wrap, jadi Home/End/PgUp/PgDn berhenti tepat di
  ujung isi meski ada paragraf panjang; lebar tampilan ikut diambil dari area render.
- TUI: warna dari config (`[ui] accent = "#rrggbb"`, `dim = "#rrggbb"`) dipakai untuk status,
  bingkai, dan hint; nilai bukan hex ditolak dengan pesan yang menyebut field-nya.
- Grant approval persisten: keputusan `Always` disimpan di `grants.json` (tulis atomik, direktori
  config pengguna) dan dimuat saat start, sehingga izin yang sudah diberikan tidak ditanyakan lagi.
- Kebijakan approval dari config kini benar-benar berlaku di TUI, teks, dan RPC (sebelumnya UI
  approver selalu bertanya dan mengabaikan `approval.mode`), termasuk `yolo` dan restricted folder.
- Audit approval: setiap keputusan gate ditulis ke transkrip sebagai entry `approval_decision`
  (tool, tier, subjek, keputusan) di mode teks, TUI, dan RPC; entry lama tanpa `tier` tetap terbaca.
- Semua approver memakai `limits.approval_timeout_secs` (default 600 detik): permintaan yang tidak
  dijawab otomatis ditolak, TUI mencatat “kedaluwarsa”, dan indikator itu tidak lagi mengklaim
  keputusan yang tidak sampai ke tool.
- RPC: approval interaktif — frame `approval_request` dengan `id`/`tool`/`tier`/`subject`, dan
  permintaan `approval` (`once`/`session`/`always`/`deny`); kebijakan config tetap berlaku, hanya
  permintaan yang butuh manusia yang dikirim ke klien.
- RPC: frame `question` dan permintaan `answer`, sehingga klien otomatis bisa menjawab tool `ask`
  (teks kosong berarti dilewati).
- Tool `ask` benar-benar bertanya di TUI: pertanyaan dari model muncul di composer, Enter menjawab,
  Esc melewati, batas tunggu 600 detik, dan jawaban dicatat di histori. Mode tanpa UI menolak
  dengan pesan jelas.
- Tool `read` menerima `offset`/`limit` dan menolak berkas biner; `grep` menerima `max_results`
  dengan penanda hasil terpotong; `ask` kini jujur menolak (belum ada UI pertanyaan) alih-alih
  mengaku menunggu jawaban.
- Pemakaian token: `Usage::accumulate` di wire, akumulasi kumulatif di sesi, `/usage` di mode
  teks dan TUI, serta permintaan `usage` di protokol RPC.
- Ringkasan compaction disimpan sebagai entry `compaction` di transkrip, dan resume memakainya
  (pesan sebelum ringkasan digantikan), sehingga sesi panjang tidak menghitung ulang ringkasan.
- Pause gate: `/pause` dan `/resume`; turn berikutnya ditolak `E_AGENT_PAUSED` dan turn berjalan
  berhenti di batas iterasi sebelum panggilan model berikutnya.
- Lock sesi: file `.lock` mencegah dua proses menulis sesi yang sama (`E_SESSION_LOCKED`),
  lock basi lebih dari satu jam diambil alih otomatis.
- Daftar dan lanjut sesi: `maicli --list-sessions`, `maicli --resume [ID]` (tanpa ID = sesi terakhir);
  mode teks, TUI, dan RPC memuat pesan lama sebagai konteks awal, dan file rusak dilewati
  saat mendaftar.
- Mode `--mode rpc`: server JSONL (`maicli-rpc/1`) dengan `ping`, `prompt`, `abort`, `model`,
  `shutdown`, event terproyeksi, dan approval berbasis config tanpa UI (`docs/rpc-v1.md`).
- Pembatalan kooperatif: `CancelToken` diteruskan ke tool; `bash` menghentikan pohon proses
  segera saat turn dibatalkan, dan turn yang sedang menunggu stream model berhenti dengan
  `stop_reason=aborted`.
- TUI: histori bisa digulir (PgUp/PgDn/Home/End/↑↓), overlay bantuan F1/`?`, status
  `model · keadaan · scroll`, dan `Esc` membatalkan turn yang berjalan.
- Workspace Rust `TUI/` dengan sembilan crate: `tui-wire`, `tui-context`, `tui-provider`,
  `tui-session`, `tui-approval`, `tui-config`, `tui-tools`, `tui-core`, `tui-app`.
- Binary `maicli` dengan mode `tui`, mode `text`, `--prompt` sekali jalan, dan `config show|path`.
- Klien gateway OpenAI-compatible: pembaca SSE tahan chunk terpotong, perakitan `tool_call`
  terfragmentasi, pemetaan `finish_reason`, retry 3 percobaan dengan jitter, taksonomi error stabil.
- Resolusi id model berlapis (persis → case-insensitive → tanpa prefiks provider → substring unik)
  dan cache discovery dengan TTL.
- Turn loop dua tingkat dengan antrean steering/follow-up dan aturan `stopReason`
  (`stop` ≡ `toolUse`, `length` tidak menjalankan tool).
- Crate `tui-context`: estimasi token, anggaran window (reserve 16.384 / keep 20.000 / band 0,8),
  pruning hasil tool lama, dan rencana compaction yang menjaga pasangan tool_use/tool_result.
- Compaction otomatis sebelum panggilan model (maks 2 per turn) plus `/compact` manual.
- Delapan tool v1: `read`, `write`, `edit`, `glob`, `grep`, `bash`, `todo`, `ask`.
- `bash` menghentikan seluruh pohon proses saat timeout (`taskkill /T` sebelum kill induk).
- Slash command v1: `/help`, `/model`, `/compact`, `/tools`, `/session`, `/quit` (mode teks dan TUI).
- Gerbang approval: 4 mode × 3 tier, kebijakan per-tool, folder terlarang, grant sesi.
- Store sesi JSONL v1 dengan tulis atomik, toleransi baris terakhir terpotong, dan fork.
- Config berlapis (default → global → proyek) dengan provenance dan redaksi kunci API.
- `check.ps1` (fmt, clippy, test, deny, aturan dependensi) dan `deny.toml`.

### Fixed
- lapisan: katalog tidak lagi bergantung tui-wire (aturan check.sh); test
  Windows-only diberi guard cfg; test bash/extension portabel (sleep/sh -c).
- Skema entry sesi kini **ketat**: field asing ditolak (`deny_unknown_fields`); hanya baris terakhir yang
  benar-benar terpotong (EOF) yang diabaikan sebagai sisa crash, sedangkan kesalahan data tetap
  `E_SESSION_CORRUPT`.
- `grants.json` dengan versi skema lebih baru kini ditolak dengan pesan jelas, bukan dibaca dengan
  asumsi versi lama (konsisten dengan penjagaan versi sesi).
- **Deadline turn** kini juga menghentikan tool yang sedang berjalan: penjaga deadline membatalkan turn,
  dan eksekusi setiap tool di-`select!` dengan cancel token (sebelumnya deadline hanya dicek sebelum
  panggilan model).
- Turn yang dibatalkan **saat menunggu approval** kini berhenti cepat: keputusan approval di-`select!`
  dengan cancel token (batal → deny + notice), bukan menggantung sampai batas waktu approval.
- Folder terlarang kini juga menutup tool `exec`: subjek `bash` berupa command string, sehingga path terlarang
  di dalam perintah dicocokkan sebagai substring (tanpa peduli huruf besar), bukan hanya sebagai path. Diuji
  untuk read/grep/write/edit/bash dan terhadap grant `always`.
- Turn yang dibatalkan kini mengirim **`message_end` sintetis** (`stop_reason=aborted`) agar pasangan event
  `message_start`/`message_end` tidak putus saat stream dihentikan — ditemukan lewat golden baru `abort`.
- Naskah bantuan/status TUI dikoreksi: F1 menyebut semua tombol yang dipakai (Ctrl+K, Ctrl+E, F2, Tab/Enter,
  ↑/↓) dan semantik Ctrl+C yang sebenarnya; hint saat sibuk tidak lagi menulis “Ctrl+C keluar”.
- Dokumen RPC dikoreksi: saat turn berjalan dilayani juga frame `approval` dan `answer`, bukan
  hanya abort/ping/shutdown (sebelumnya dokumen menyatakan sebaliknya).
- `bash` yang timeout/dibatalkan kini mengembalikan **output parsial** yang sudah dihasilkan (ditandai
  `[output parsial]` dan `truncated=true`), bukan hanya pesan timeout — sesuai C-04 kriteria 2. Pipe
  ditutup setelah proses dibunuh dan pembacaan dibatasi 2 detik agar turn tidak menggantung.
- `maicli config set` dengan kunci asing kini memakai pesan **“kunci config tidak dikenal: unknown field …”**
  alih-alih awalan “TOML parse error” yang menyesatkan (file tetap tidak ditulis, exit 2).
- Error provider kini tetap memasangkan event `message_start` dengan `message_end`: dibuat pesan
  sintetis ber-`stop_reason: error` (berisi alasan) sehingga UI tidak menggantung menunggu akhir pesan
  yang tidak pernah datang. Skenario golden `error-provider` mengunci urutan ini.
- Tool yang **tidak dikenal** kini mengirim event `tool_execution_end` (error) ke UI, bukan hanya
  menulis `tool_result` yang tidak terlihat; timeline tidak lagi tampak diam saat model memanggil
  tool asing (C-04 kriteria 3).
- Pemicu compaction kini memakai **max(estimasi lokal, usage provider)** sesuai RENCANA C-07; sebelumnya
  hanya estimasi lokal, sehingga konteks yang dilaporkan gateway lebih besar bisa terlambat diringkas.
  Ditambah test hysteresis: setelah compaction, turn berikutnya tidak langsung meringkas lagi.
- Dokumen RPC dikoreksi: saat turn berjalan dilayani juga frame `approval` dan `answer`, bukan
  hanya abort/ping/shutdown (sebelumnya dokumen menyatakan sebaliknya).
- SSE: `\r` di ujung satu chunk dan `\n` di chunk berikutnya tidak lagi menjadi dua newline yang
  memalsukan batas event; baris `data:` kosong tidak diteruskan sebagai payload JSON kosong.
- Penulisan `grants.json` dipindah ke `spawn_blocking` sesuai aturan async proyek (fs sinkron tidak
  dijalankan di dalam future); kegagalan menulis tidak lagi berisiko menjatuhkan turn dan diuji.
- Perhitungan lipat baris TUI memakai `unicode-width`, jadi teks CJK dan emoji dihitung 2 kolom;
  gulir tidak lagi berhenti terlalu jauh dari ujung isi pada teks non-Latin.
- Kegagalan membuka/membuat sesi tidak lagi hilang diam-diam: mode teks, TUI, dan RPC memberi tahu
  pengguna ("sesi tidak tersimpan: …") alih-alih berjalan tanpa persistensi tanpa penjelasan.
- Literal non-ASCII di test diganti escape Unicode (`\u{…}`) agar berkas tetap ASCII; penulisan
  ulang lewat PowerShell sebelumnya merusak karakter CJK/emoji menjadi mojibake.
