"""Klien RPC maicli v1 — Python 3, tanpa dependensi eksternal."""

from __future__ import annotations

import json
from typing import Any, Callable, Iterable, TextIO


class Frame(dict[str, Any]):
    """Satu frame JSON dari server (field tambahan dibaca apa adanya)."""

    @property
    def type(self) -> str:
        return str(self.get("type", ""))

    @property
    def id(self) -> str:
        return str(self.get("id", ""))


class Client:
    """Baca frame dari `reader`, tulis permintaan ke `writer`.

    `on_question`, `on_approval`, dan `on_event` opsional; balasan interaktif
    dikirim otomatis supaya turn tidak menggantung.
    """

    def __init__(
        self,
        reader: Iterable[str],
        writer: TextIO,
        *,
        on_question: Callable[[str, str], str] | None = None,
        on_approval: Callable[[str, str, str, str], str] | None = None,
        on_event: Callable[[Frame], None] | None = None,
    ) -> None:
        selfreader = iter(reader)
        self._reader = selfreader
        self._writer = writer
        self._next = 0
        self.on_question = on_question
        self.on_approval = on_approval
        self.on_event = on_event

    def _new_id(self) -> str:
        self._next += 1
        return f"py-{self._next}"

    def _send(self, value: dict[str, Any]) -> None:
        self._writer.write(json.dumps(value) + "\n")
        self._writer.flush()

    def _recv(self) -> Frame:
        for baris in self._reader:
            baris = baris.strip()
            if not baris:
                continue
            return Frame(json.loads(baris))
        raise EOFError("maicli menutup kanal")

    def prompt(self, message: str) -> Frame:
        """Jalankan satu turn; kembalikan frame `result`."""
        ident = self._new_id()
        self._send({"type": "prompt", "id": ident, "message": message})
        while True:
            frame = self._recv()
            if frame.type == "question":
                jawaban = ""
                if self.on_question:
                    jawaban = self.on_question(frame.id, str(frame.get("question", "")))
                self._send({"type": "answer", "id": frame.id, "text": jawaban})
            elif frame.type == "approval_request":
                keputusan = "deny"
                if self.on_approval:
                    keputusan = self.on_approval(
                        frame.id,
                        str(frame.get("tool", "")),
                        str(frame.get("tier", "")),
                        str(frame.get("subject", "")),
                    )
                self._send({"type": "approval", "id": frame.id, "decision": keputusan})
            elif frame.type == "event" and self.on_event:
                self.on_event(frame)
            elif frame.type == "result" and frame.id == ident:
                return frame
            elif frame.type == "error" and frame.id == ident:
                raise RuntimeError(str(frame.get("message", "galat maicli")))

    def ping(self) -> Frame:
        ident = self._new_id()
        self._send({"type": "ping", "id": ident})
        while True:
            frame = self._recv()
            if frame.type == "pong" and frame.id == ident:
                return frame

    def shutdown(self) -> None:
        self._send({"type": "shutdown"})
