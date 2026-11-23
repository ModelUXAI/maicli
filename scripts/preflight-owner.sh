#!/usr/bin/env bash
# preflight-owner.sh — cek prasyarat item eksternal sebelum serah terima.
# Tidak pernah mencetak nilai kunci. Jalankan: bash scripts/preflight-owner.sh
set -u
cd "$(dirname "$0")/.."

ok=0; kurang=0
cek() { # nama, kondisi, petunjuk
  local nama="$1" kondisi="$2" petunjuk="$3"
  if eval "$kondisi" >/dev/null 2>&1; then
    printf 'OK   %s\n' "$nama"; ok=$((ok+1))
  else
    printf 'KURANG %s — %s\n' "$nama" "$petunjuk"; kurang=$((kurang+1))
  fi
}

cek "binary debug" "[ -x target/debug/maicli ]" "cargo build"
cek "binary release" "[ -x target/release/maicli ]" "cargo build --release"
cek "MODELUXAI_API_KEY" "[ -n \"\${MODELUXAI_API_KEY:-}\" ]" "export MODELUXAI_API_KEY / key-import"
cek "git remote" "git remote -v | grep -q ." "tambahkan remote sebelum push"
cek "relay key" "[ -f \"\${MAICLI_DATA_DIR:-\$HOME/.local/share/maicli}/relay-key\" ]" "jalankan --mode relay sekali"
cek "tag v0.1.0" "git tag --list v0.1.0 | grep -q ." "buat tag saat rilis"
cek "secret Apple" "[ -n \"\${APPLE_CERT_P12:-}\" ]" "set secret hanya di CI/host"

echo
echo "OK=$ok KURANG=$kurang"
[ "$kurang" -eq 0 ]
