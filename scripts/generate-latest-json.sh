#!/usr/bin/env bash
# Aggregate per-platform updater signatures from a GitHub Release into a
# single `latest.json` manifest that tauri-plugin-updater can consume.
#
# Usage: VERSION=1.3.3 bash scripts/generate-latest-json.sh > latest.json
#
# Safety notes:
#  - $VERSION is validated against a strict regex before any shell
#    substitution so a malicious workflow input cannot inject commands.
#  - The JSON output is built entirely by `jq` from typed `--arg` values;
#    no string concatenation of user-controlled data into JSON.
set -euo pipefail

if [[ -z "${VERSION:-}" ]]; then
    echo "ERROR: VERSION env var required (e.g. 1.3.3)" >&2
    exit 1
fi

# Accept only `MAJOR.MINOR.PATCH[-prerelease]` — reject anything else.
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.]+)?$ ]]; then
    echo "ERROR: VERSION '$VERSION' is not a valid semver" >&2
    exit 1
fi

REPO="${GITHUB_REPOSITORY:-Sebdev43/KaraokeTyper}"
TAG="v${VERSION}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Pull the list of assets for this release.
ASSETS_JSON="$TMP/assets.json"
gh release view "$TAG" --repo "$REPO" --json assets,body > "$ASSETS_JSON"

# Helper: find the first asset whose name matches a regex and print the pair
# "<api_url> <browser_download_url>" separated by a tab. Empty if no match.
find_asset() {
    local pattern="$1"
    jq -r --arg p "$pattern" \
        '.assets[] | select(.name | test($p)) | "\(.url)\t\(.browser_download_url)"' \
        "$ASSETS_JSON" | head -n1
}

# Helper: download a .sig asset by regex and return its contents.
fetch_signature() {
    local pattern="$1"
    local pair api_url
    pair=$(find_asset "$pattern")
    if [[ -z "$pair" ]]; then
        echo ""
        return
    fi
    api_url=${pair%%$'\t'*}
    curl -fsSL -H "Accept: application/octet-stream" \
        -H "Authorization: Bearer ${GH_TOKEN:-${GITHUB_TOKEN:-}}" \
        "$api_url" 2>/dev/null || echo ""
}

# Extract browser_download_url (second column of the tab-separated pair).
find_browser_url() {
    local pattern="$1"
    local pair
    pair=$(find_asset "$pattern")
    if [[ -z "$pair" ]]; then echo ""; return; fi
    printf '%s' "${pair#*$'\t'}"
}

LINUX_URL=$(find_browser_url '\.AppImage$')
LINUX_SIG=$(fetch_signature '\.AppImage\.sig$')

WIN_URL=$(find_browser_url '_x64-setup\.exe$|_x64-setup\.nsis\.zip$')
WIN_SIG=$(fetch_signature '_x64-setup\.(exe|nsis\.zip)\.sig$')

DARWIN_URL=$(find_browser_url '\.app\.tar\.gz$')
DARWIN_SIG=$(fetch_signature '\.app\.tar\.gz\.sig$')

PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
NOTES=$(jq -r '.body // ""' "$ASSETS_JSON" | head -c 2000)

# Build platforms object one key at a time, entirely via jq (no JSON
# string concat from user-controlled values).
PLATFORMS='{}'
add_platform() {
    local key="$1" sig="$2" url="$3"
    if [[ -n "$sig" && -n "$url" ]]; then
        PLATFORMS=$(jq -c --arg k "$key" --arg s "$sig" --arg u "$url" \
            '. + { ($k): { signature: $s, url: $u } }' <<<"$PLATFORMS")
    fi
}

add_platform "linux-x86_64"   "$LINUX_SIG"  "$LINUX_URL"
add_platform "windows-x86_64" "$WIN_SIG"    "$WIN_URL"
# Apple Silicon only, and deliberately not `darwin-x86_64`.
#
# The release workflow builds macOS on an arm64 runner and publishes a single
# aarch64 bundle; the Intel job was dropped when the macos-13 runner went away.
# Announcing that bundle under `darwin-x86_64` — which this script used to do —
# hands an Intel Mac an update it cannot run. Offering nothing leaves such a
# user on the version they already have, which is the honest outcome until an
# Intel bundle exists again.
add_platform "darwin-aarch64" "$DARWIN_SIG" "$DARWIN_URL"

jq -n --arg ver "$VERSION" --arg notes "$NOTES" --arg pub "$PUB_DATE" \
    --argjson plats "$PLATFORMS" \
    '{version: $ver, notes: $notes, pub_date: $pub, platforms: $plats}'
