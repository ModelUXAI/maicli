#!/usr/bin/env python3
"""Cek tautan relatif di semua Markdown repo (tanpa dependensi eksternal)."""

from __future__ import annotations

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
SKIP_DIRS = {".git", "target", "node_modules", "dist"}


def main() -> int:
    rusak: list[tuple[str, str]] = []
    for root, dirs, files in os.walk(ROOT):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for nama in files:
            if not nama.endswith(".md"):
                continue
            path = os.path.join(root, nama)
            try:
                isi = open(path, encoding="utf-8").read()
            except OSError:
                continue
            for m in LINK.finditer(isi):
                link = m.group(1).split("#")[0].strip()
                if not link or link.startswith(("http://", "https://", "mailto:", "#")):
                    continue
                if link.startswith("<") and link.endswith(">"):
                    link = link[1:-1]
                target = os.path.normpath(os.path.join(root, link))
                if not os.path.exists(target):
                    rusak.append((os.path.relpath(path, ROOT), link))
    if rusak:
        for path, link in rusak:
            print(f"{path}: tautan rusak -> {link}")
        return 1
    print("tautan dokumentasi ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
