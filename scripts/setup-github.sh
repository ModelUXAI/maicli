#!/usr/bin/env bash
# Siapkan repo GitHub untuk alur open-source:
#   PR wajib → required checks → squash merge → release-plz → tag → biner.
#
# Sekali jalan (butuh gh CLI + admin repo):
#   REPO=ModelUXAI/maicli ./scripts/setup-github.sh
#
# Idempoten: ruleset yang sudah ada diperbarui, label dibuat/diubah.
set -euo pipefail

REPO="${REPO:-ModelUXAI/maicli}"
RULESET="main-protection"

command -v gh >/dev/null || {
  echo "gh CLI tidak ditemukan: https://cli.github.com/" >&2
  exit 1
}
gh auth status >/dev/null 2>&1 || {
  echo "jalankan 'gh auth login' (scope repo + admin:org bila perlu)" >&2
  exit 1
}

echo "== repo: $REPO"

# Squash-only + auto-merge + hapus branch setelah merge.
gh api -X PATCH "repos/$REPO" \
  -F allow_squash_merge=true \
  -F allow_merge_commit=false \
  -F allow_rebase_merge=false \
  -F delete_branch_on_merge=true \
  -F allow_auto_merge=true >/dev/null
echo "   merge: squash-only, auto-merge, delete branch"

# GITHUB_TOKEN boleh membuat PR (release-plz) tetapi default permission read;
# setiap workflow menaikkan permission sendiri sesuai kebutuhan.
gh api -X PUT "repos/$REPO/actions/permissions/workflow" \
  -F default_workflow_permissions=read \
  -F can_approve_pull_request_reviews=true >/dev/null
echo "   actions: default read, PR creation allowed"

PAYLOAD=$(cat <<'JSON'
{
  "name": "main-protection",
  "target": "branch",
  "enforcement": "active",
  "conditions": {
    "ref_name": {
      "include": ["~DEFAULT_BRANCH"],
      "exclude": []
    }
  },
  "bypass_actors": [
    {
      "actor_id": 5,
      "actor_type": "RepositoryRole",
      "bypass_mode": "pull_request"
    }
  ],
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_linear_history" },
    {
      "type": "pull_request",
      "parameters": {
        "required_approving_review_count": 1,
        "dismiss_stale_reviews_on_push": true,
        "require_code_owner_review": true,
        "require_last_push_approval": false,
        "required_review_thread_resolution": true
      }
    },
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [
          { "context": "fmt" },
          { "context": "clippy (ubuntu-latest)" },
          { "context": "clippy (windows-latest)" },
          { "context": "test (ubuntu-latest)" },
          { "context": "test (windows-latest)" },
          { "context": "build-release" },
          { "context": "deny" },
          { "context": "msrv" },
          { "context": "actionlint" },
          { "context": "pr-title" },
          { "context": "dependency-review" }
        ]
      }
    }
  ]
}
JSON
)

ID=$(gh api "repos/$REPO/rulesets" --jq ".[] | select(.name == \"$RULESET\") | .id" | head -1 || true)
if [ -n "$ID" ]; then
  gh api -X PUT "repos/$REPO/rulesets/$ID" --input - <<<"$PAYLOAD" >/dev/null
  echo "   ruleset diperbarui (id $ID)"
else
  gh api -X POST "repos/$REPO/rulesets" --input - <<<"$PAYLOAD" >/dev/null
  echo "   ruleset dibuat"
fi

# Keamanan (best effort: sebagian endpoint hanya untuk repo publik).
gh api -X PUT "repos/$REPO/vulnerability-alerts" >/dev/null 2>&1 || \
  echo "   vulnerability alerts: lewati (perlu admin/publik)"
gh api -X PUT "repos/$REPO/automated-security-fixes" >/dev/null 2>&1 || \
  echo "   dependabot security updates: lewati (aktifkan alerts dulu)"
gh api -X PUT "repos/$REPO/private-vulnerability-reporting" >/dev/null 2>&1 || \
  echo "   private vulnerability reporting: lewati (repo privat?)"

label() {
  gh label create "$1" --color "$2" --description "$3" --force >/dev/null 2>&1 || true
}
label feat "0e8a16" "Fitur baru"
label fix "d73a4a" "Perbaikan bug"
label docs "0075ca" "Dokumentasi"
label refactor "c5def5" "Tanpa perubahan perilaku"
label perf "fbca04" "Performa"
label test "bfd4f2" "Test"
label ci "5319e7" "CI/CD"
label security "b60205" "Keamanan"
label dependencies "0366d6" "Dependabot"
label release "1d76db" "Rilis"
label "good first issue" "7057ff" "Ramah kontributor baru"
label "help wanted" "008672" "Butuh bantuan"

echo "== selesai"
echo "Aktifkan juga di UI: Settings → Code security → Secret scanning + Push protection."
echo "Branch protection kini: PR + 1 review + checks di atas + linear history."
echo "Admin hanya boleh bypass aturan PR (bukan status checks); CI tetap wajib."
