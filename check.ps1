# check.ps1 — gerbang mutu proyek (Windows).
#
# Menjalankan lima gerbang yang sama dengan AGENTS.md §7. Tidak ada gerbang
# yang boleh dilunakkan untuk mengakali kegagalan.
#
# Catatan: $ErrorActionPreference sengaja 'Continue'. Cargo menulis progres ke
# stderr, dan PowerShell 5.1 menganggap NativeCommandError sebagai terminating
# error bila disetel 'Stop'. Kebenaran hasil tetap dibaca dari $LASTEXITCODE.

$ErrorActionPreference = 'Continue'
$failed = @()

function Invoke-Gate {
    param([string]$Name, [scriptblock]$Body)
    Write-Host ""
    Write-Host "== $Name"
    & $Body 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        $script:failed += $Name
        Write-Host "GAGAL: $Name (exit $LASTEXITCODE)"
    }
}

function Test-DependencyRules {
    $violations = 0
    foreach ($crate in @('tui-wire', 'tui-provider', 'tui-session', 'tui-approval', 'tui-config', 'tui-tools', 'tui-core')) {
        $tree = cargo tree -p $crate --prefix none 2>$null
        if ($tree -match '(?m)^tui-app v') {
            Write-Host "$crate bergantung pada tui-app (dilarang)"
            $violations++
        }
    }
    if ($violations -gt 0) { return $false }
    Write-Host "crate inti tidak bergantung pada tui-app"
    return $true
}

Invoke-Gate 'cargo fmt --check' { cargo fmt --all -- --check }
Invoke-Gate 'cargo clippy -D warnings' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Gate 'cargo test' { cargo test --workspace }
Invoke-Gate 'cargo deny check' { cargo deny check --hide-inclusion-graph }

Write-Host ""
Write-Host "== aturan dependensi"
if (-not (Test-DependencyRules)) {
    $failed += 'aturan dependensi'
    Write-Host "GAGAL: aturan dependensi"
}

Write-Host ""
if ($failed.Count -gt 0) {
    Write-Host ("GERBANG GAGAL: " + ($failed -join ', '))
    exit 1
}
Write-Host "SEMUA GERBANG HIJAU"
exit 0
