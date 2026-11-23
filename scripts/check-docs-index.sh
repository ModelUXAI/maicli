#!/usr/bin/env bash
# check-docs-index.sh — pastikan setiap docs/*.md tercantum di docs/README.md.
set -u
cd "$(dirname "$0")/.."
# Git menyelesaikan symlink ke path fisik (/mnt/build/...), sedangkan $PWD bisa
# berupa path logis (/home/linux/...). Pakai path fisik agar check-ignore tidak
# mengira repo bukan working tree.
SAFE_DIR="$(pwd -P)"
missing=0
for f in docs/*.md; do
  nama=$(basename "$f")
  [ "$nama" = "README.md" ] && continue
  # Dokumen internal yang di-.gitignore tidak wajib masuk indeks publik.
  if git -c safe.directory="$SAFE_DIR" check-ignore -q "$f"; then
    continue
  fi
  if ! grep -q "$nama" docs/README.md; then
    echo "tidak ada di indeks: $nama"
    missing=$((missing+1))
  fi
done
if [ "$missing" -gt 0 ]; then
  echo "GAGAL: $missing dokumen tidak terindeks"
  exit 1
fi
echo "indeks dokumentasi lengkap"
