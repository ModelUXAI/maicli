# SDK RPC lintas bahasa (Go / Python / TypeScript)

Ketiga SDK adalah pembungkus tipis di atas **maicli RPC v1**: satu objek JSON
per baris di stdin/stdout. Tidak ada dependensi eksternal; cukup toolchain
bahasa masing-masing.

| Bahasa | Path | Test |
|---|---|---|
| Go | `sdk/go/maicli/` | `cd sdk/go && go test ./...` |
| Python 3 | `sdk/python/maicli_rpc.py` | `cd sdk/python && python3 -m unittest -v` |
| TypeScript/Node | `sdk/typescript/src/client.mjs` + `client.d.ts` | `cd sdk/typescript && npm test` |

Semua klien menyediakan `prompt`, `ping`, `shutdown`, dan callback untuk
`question`, `approval_request`, serta `event`. Frame `prompt_result` dan
`session_settled` diteruskan apa adanya ke callback event/result; klien v1
tetap bisa dipakai terhadap server v2.

## Kebijakan semver

- Versi SDK mengikuti **versi rilis maicli** (`[workspace.package] version`);
  rilis `vX.Y.Z` menerbitkan ketiga SDK dengan nomor `X.Y.Z`.
- Protokol **v1 tidak akan diubah secara incompatible**; field baru bersifat
  tambahan dan diabaikan klien lama.
- Perubahan yang memecah klien v1 hanya boleh lewat versi mayor maicli.
- Klien v2 (negosiasi + chunk 64 MiB) diaktifkan lewat negosiasi; dukungan penuh
  menyusul setelah E2E lintas bahasa.

## Contoh minimal (Python)

```python
from maicli_rpc import Client
import subprocess

proc = subprocess.Popen(["maicli", "--mode", "rpc"], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, text=True)
c = Client(proc.stdout, proc.stdin)
print(c.prompt("ringkas README ini")["stop_reason"])
c.shutdown()
```
