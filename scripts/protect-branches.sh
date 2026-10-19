#!/usr/bin/env bash
# Apply branch protection to `master` and `dev`.
#
# GitHub refuses branch protection on a private repository without a paid plan,
# so this cannot run until the repository is public — hence a script rather than
# a setting applied once by hand: run it the moment visibility changes, and
# again after any workflow rename, since the required checks are matched by the
# job names below.
#
# Usage: bash scripts/protect-branches.sh
set -euo pipefail

REPO="${GITHUB_REPOSITORY:-Sebdev43/KaraokeTyper}"

# Exactly the `name:` of each job that must pass. The two jobs whose name ends
# in "(informational)" are deliberately absent: they report, they do not gate.
CHECKS=(
    "Rust — fmt, clippy, tests"
    "Frontend — build and lint"
    "check-dev-files"
    "Rust — security advisories"
)

visibility=$(gh repo view "$REPO" --json visibility -q .visibility)
if [[ "$visibility" != "PUBLIC" ]]; then
    echo "ERROR: $REPO is $visibility; GitHub rejects branch protection on a" >&2
    echo "       private repository without GitHub Pro. Make it public first." >&2
    exit 1
fi

# The payload is built by jq and sent as a JSON body. `gh api -f` sends every
# value as a string, and this endpoint rejects `"true"` where it wants `true`
# and `""` where it wants `null`, with a 422 that names neither the field nor
# the branch.
protect() {
    local branch="$1"
    echo "→ $branch"

    jq -n --argjson contexts "$(printf '%s\n' "${CHECKS[@]}" | jq -R . | jq -s .)" \
        '{
            required_status_checks: { strict: true, contexts: $contexts },
            enforce_admins: true,
            required_pull_request_reviews: {
                required_approving_review_count: 0,
                dismiss_stale_reviews: true
            },
            restrictions: null,
            allow_force_pushes: false,
            allow_deletions: false,
            required_conversation_resolution: true,
            required_linear_history: false
        }' |
        gh api -X PUT "repos/$REPO/branches/$branch/protection" \
            -H "Accept: application/vnd.github+json" --input - >/dev/null
}

# `master` and `dev` get the same rules: a pull request, green checks, no force
# push, no deletion. A single maintainer cannot approve their own pull request,
# so the required review count stays at zero — the checks are the gate.
protect master
protect dev

echo
echo "Protection in place:"
for branch in master dev; do
    gh api "repos/$REPO/branches/$branch/protection" \
        --jq "\"  $branch: checks=\(.required_status_checks.contexts | join(\", \")) | force_push=\(.allow_force_pushes.enabled) | deletions=\(.allow_deletions.enabled)\""
done
