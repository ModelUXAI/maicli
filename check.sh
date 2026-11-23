#!/usr/bin/env bash
# check.sh — gerbang mutu proyek (padanan check.ps1 untuk shell POSIX).
#
# Menjalankan lima gerbang yang sama dengan AGENTS.md §7. Target utama proyek
# adalah Windows-native; skrip ini disediakan untuk CI/jaringan non-Windows dan
# wajib menghasilkan gerbang yang sama, bukan versi yang lebih longgar.
#
# Pemakaian: bash check.sh
set -u
# E2E relay/subagent sensitif terhadap beban paralel; serialkan test agar gate
# deterministik. Bisa ditimpa: RUST_TEST_THREADS=8 bash check.sh
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"
failed=()

invoke_gate() {
    local name="$1"
    shift
    echo
    echo "== $name"
    if ! "$@"; then
        failed+=("$name")
        echo "GAGAL: $name"
    fi
}

dependency_rules() {
    local violations=0
    for crate in tui-wire tui-provider tui-session tui-approval tui-config tui-tools tui-core; do
        if cargo tree -p "$crate" --prefix none 2>/dev/null | grep -q '^tui-app v'; then
            echo "$crate bergantung pada tui-app (dilarang)"
            violations=$((violations + 1))
        fi
    done
    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    echo "crate inti tidak bergantung pada tui-app"
    return 0
}

invoke_gate 'indeks dokumentasi' bash scripts/check-docs-index.sh
invoke_gate 'tautan dokumentasi' python3 scripts/check_doc_links.py
invoke_gate 'cargo fmt --check' cargo fmt --all -- --check
invoke_gate 'cargo clippy -D warnings' cargo clippy --workspace --all-targets -- -D warnings
invoke_gate 'cargo test' cargo test --workspace
invoke_gate 'cargo deny check' cargo deny check --hide-inclusion-graph

echo
echo "== aturan dependensi"
if ! dependency_rules; then
    failed+=('aturan dependensi')
    echo "GAGAL: aturan dependensi"
fi

echo
if [ "${#failed[@]}" -gt 0 ]; then
    joined=$(printf '%s, ' "${failed[@]}")
    echo "GERBANG GAGAL: ${joined%, }"
    exit 1
fi
echo "SEMUA GERBANG HIJAU"
exit 0
