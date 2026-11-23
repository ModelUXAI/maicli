"""Test klien RPC maicli (unittest stdlib)."""

import io
import json
import unittest

from maicli_rpc import Client


class TestClient(unittest.TestCase):
    def test_prompt_roundtrip(self):
        frames = [
            {"type": "ready", "protocol": "maicli-rpc/1", "model": "mock"},
            {"type": "event", "event": {"event": "agent_start"}},
            {"type": "prompt_result", "id": "py-1", "status": "ok"},
            {"type": "result", "id": "py-1", "stop_reason": "stop", "turns": 1},
        ]
        reader = iter([json.dumps(f) + "\n" for f in frames])
        out = io.StringIO()
        events = []
        client = Client(reader, out, on_event=events.append)
        hasil = client.prompt("halo")
        self.assertEqual(hasil["stop_reason"], "stop")
        self.assertEqual(len(events), 1)
        self.assertIn('"message": "halo"', out.getvalue())


if __name__ == "__main__":
    unittest.main()
