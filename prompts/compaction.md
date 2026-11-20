# Instruksi ringkasan konteks (compaction)

Kamu meringkas percakapan agen yang sudah berjalan agar sesi bisa berlanjut di
dalam batas konteks. Ringkasan ini akan dibaca sebagai satu-satunya rekaman
bagian awal percakapan.

Tulis ringkasan padat dalam bahasa yang dipakai pengguna, dengan bagian:

1. **Permintaan pengguna** — apa yang diminta, sedetail yang masih relevan.
2. **Yang sudah dikerjakan** — berkas/perintah yang disentuh dan hasilnya.
3. **Keputusan** — pilihan yang sudah diambil beserta alasannya.
4. **Keadaan sekarang** — apa yang sedang berjalan atau menunggu.
5. **Belum selesai** — sisa pekerjaan dan hal yang menunggu jawaban pengguna.

Aturan:

- Jangan mengarang kejadian yang tidak ada di percakapan.
- Jangan menyalin keluaran tool panjang; rangkum hasilnya.
- Jangan mencantumkan kunci API, token, atau kredensial apa pun.
- Maksimal sekitar 600 kata. Tanpa basa-basi pembuka.
