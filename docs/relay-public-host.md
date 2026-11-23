# Host relay publik (runbook)

Tujuan: menjalankan `maicli --mode relay` di host publik di belakang nginx
TLS + WebSocket, tanpa membuka port relay ke internet.

## 1. Prasyarat

- Biner `maicli` (rilis linux-x64/arm64) di `/usr/local/bin/maicli`.
- Domain, mis. `relay.example.com`, A/AAAA ke host.
- Sertifikat TLS (Let's Encrypt atau CA internal).
- User sistem `maicli` tanpa shell login.

## 2. Kunci + data

```bash
sudo install -d -m 0700 -o maicli -g maicli /var/lib/maicli
sudo -u maicli sh -c 'umask 077; printf "%s" "GANTI-KUNCI-PANJANG" > /var/lib/maicli/relay-key'
sudo chmod 0600 /var/lib/maicli/relay-key
```

Jangan menaruh kunci di unit/systemd environment yang terbaca semua user.

## 3. Unit systemd

```ini
[Unit]
Description=maicli relay
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=maicli
Group=maicli
Environment=MAICLI_DATA_DIR=/var/lib/maicli
Environment=MODELUXAI_API_KEY_FILE=/etc/maicli/api-key
ExecStart=/usr/local/bin/maicli --mode relay --port 7475 --set collab.remote_url=https://relay.example.com
Restart=on-failure
RestartSec=2
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/maicli
LimitNOFILE=4096

[Install]
WantedBy=multi-user.target
```

Catatan: bila biner belum mendukung `*_FILE`, gunakan `EnvironmentFile=` dengan
file 0600 milik user `maicli`; jangan menaruh kunci di unit.

## 4. nginx (TLS + WS)

```nginx
server {
    listen 443 ssl http2;
    server_name relay.example.com;

    ssl_certificate     /etc/letsencrypt/live/relay.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/relay.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:7475;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 3600s;
    }
}
```

`proxy_read_timeout` besar diperlukan untuk WebSocket idle. Jangan menambahkan
`proxy_buffering on` pada lokasi WS.

## 5. Verifikasi

```bash
systemctl daemon-reload
systemctl enable --now maicli-relay
curl -fsS https://relay.example.com/api/relay-info | head -c 200
# buka https://relay.example.com/ di browser, masukkan room+kunci
```

Rotasi kunci (dari mesin yang punya kunci lama):

```bash
curl -fsS -X POST -H "x-relay-key: KUNCI-LAMA" -H "x-relay-room: default" \
  https://relay.example.com/api/relay/revoke
```

Simpan kunci baru ke `/var/lib/maicli/relay-key` (0600) lalu restart unit.

## 6. Jangan

- Jangan bind relay ke `0.0.0.0` tanpa reverse proxy TLS.
- Jangan commit kunci/domain internal ke repo.
- Jangan memakai `docker-compose.upstream.yml` atau stack lain untuk relay.
