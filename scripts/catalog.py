#!/usr/bin/env python3
"""Check (or refresh) the model catalogue against HuggingFace.

The catalogue pins a SHA-256 and an exact byte size for every downloadable model file.
Those values were originally typed in by hand, which is exactly the kind of thing that
is wrong 1% of the time and impossible to notice: a bad hash does not misbehave, it
makes a download silently refuse to install, months later, on someone else's machine.

HuggingFace exposes the content SHA-256 of every LFS file as `lfs.oid` in its tree API,
so the values can be checked rather than trusted.

    scripts/catalog.py check     # compare the committed catalogue to upstream, write nothing
    scripts/catalog.py update    # rewrite the hashes and sizes from upstream

`check` is the mode that matters: it exits non-zero on any drift, so it can run in CI or
before a release without the risk of quietly rewriting what it was asked to verify.
`update` is for when a model is added or upstream republishes a file.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

CATALOG = Path(__file__).resolve().parent.parent / "src-tauri" / "src" / "catalog" / "catalog.json"
HF_API = "https://huggingface.co/api/models/{repo}/tree/{revision}"

# Non-LFS files are hashed by downloading them; this keeps that from ever
# becoming a multi-gigabyte fetch by accident.
SMALL_FILE_LIMIT = 4 * 1024 * 1024

# `https://huggingface.co/<repo>/resolve/<revision>/<path>`
HF_URL = re.compile(r"^https://huggingface\.co/(?P<repo>[^/]+/[^/]+)/resolve/(?P<revision>[^/]+)/(?P<path>.+)$")


def parse_hf_url(url: str):
    """Split a HuggingFace resolve URL, or return None if it is not one."""
    m = HF_URL.match(url)
    return m.groupdict() if m else None


def fetch_tree(repo: str, revision: str) -> dict[str, dict]:
    """Map path -> entry for one repo revision, following pagination."""
    entries: dict[str, dict] = {}
    url = HF_API.format(repo=repo, revision=revision) + "?recursive=true"
    while url:
        req = urllib.request.Request(url, headers={"User-Agent": "KaraokeTyper-catalog/1"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            for e in json.load(resp):
                entries[e["path"]] = e
            # HF paginates with a Link header; follow it so large repos are complete.
            link = resp.headers.get("Link", "")
            nxt = re.search(r'<([^>]+)>;\s*rel="next"', link)
            url = nxt.group(1) if nxt else None
    return entries


def upstream_for(url: str, cache: dict) -> tuple[str, int]:
    """Return (sha256, size_bytes) for a HuggingFace file URL."""
    parts = parse_hf_url(url)
    if not parts:
        raise ValueError(f"not a HuggingFace resolve URL: {url}")

    key = (parts["repo"], parts["revision"])
    if key not in cache:
        cache[key] = fetch_tree(*key)

    entry = cache[key].get(parts["path"])
    if entry is None:
        raise ValueError(f"{parts['path']} not found in {parts['repo']}@{parts['revision']}")

    lfs = entry.get("lfs") or {}
    # `oid` on an LFS entry is the SHA-256 of the file content — the value we pin.
    sha = lfs.get("oid") or lfs.get("sha256")
    size = lfs.get("size") or entry.get("size")
    if sha and size:
        return sha, int(size)

    # Small files (config.json, vocab.txt) are stored as plain git blobs, so the API
    # only offers a SHA-1. Hash the content instead — exact, and cheap at this size.
    size = int(entry.get("size") or 0)
    if size > SMALL_FILE_LIMIT:
        raise ValueError(
            f"{parts['path']} has no LFS metadata and is {size} bytes — too large to hash here"
        )
    req = urllib.request.Request(url, headers={"User-Agent": "KaraokeTyper-catalog/1"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        blob = resp.read(SMALL_FILE_LIMIT + 1)
    # The hash of a small file comes from *downloading* it, so a transient wrong body
    # — an HF error page served with 200, a CDN redirect, a poisoned cache — would be
    # pinned as a perfectly well-formed hash and break every install afterwards. The
    # API already told us the exact size; insist on it.
    if len(blob) != size:
        raise ValueError(
            f"{parts['path']}: received {len(blob)} bytes, API announced {size}"
        )
    return hashlib.sha256(blob).hexdigest(), len(blob)


def iter_pinned(catalog: dict):
    """Yield (label, container, url) for every file the catalogue pins a hash for."""
    for model in catalog["models"]:
        if model.get("is_directory"):
            for f in model.get("files", []):
                yield f"{model['name']}/{f['filename']}", f, f["url"]
        elif model.get("url"):
            yield model["name"], model, model["url"]


def count_hashes(node) -> int:
    """Every `sha256` anywhere in the document, however the entry is shaped."""
    if isinstance(node, dict):
        return sum(
            (1 if k == "sha256" and v else 0) + count_hashes(v) for k, v in node.items()
        )
    if isinstance(node, list):
        return sum(count_hashes(v) for v in node)
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    if mode not in {"check", "update"}:
        print(__doc__)
        return 2

    catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
    cache: dict = {}
    drift, errors = [], []

    for label, container, url in iter_pinned(catalog):
        try:
            sha, size = upstream_for(url, cache)
        except (ValueError, urllib.error.URLError, OSError) as e:
            errors.append(f"{label}: {e}")
            continue

        pinned_sha = container.get("sha256", "")
        pinned_size = container.get("size_bytes")

        if pinned_sha != sha:
            drift.append(f"{label}: sha256 {pinned_sha or '(none)'} -> {sha}")
            if mode == "update":
                container["sha256"] = sha
        # size_bytes is only pinned for single-file models; leave bundles alone.
        if pinned_size is not None and pinned_size != size:
            drift.append(f"{label}: size {pinned_size} -> {size}")
            if mode == "update":
                container["size_bytes"] = size

    for e in errors:
        print(f"ERROR  {e}", file=sys.stderr)

    if mode == "check":
        for d in drift:
            print(f"DRIFT  {d}", file=sys.stderr)
        if drift or errors:
            print(
                f"\n{len(drift)} drift, {len(errors)} error(s) — "
                "run `scripts/catalog.py update` if upstream really changed.",
                file=sys.stderr,
            )
            return 1
        # Coverage guard: `iter_pinned` decides what to check from `is_directory`, so a
        # malformed entry could carry a hash it never looks at and still print OK.
        # Count every `sha256` in the document and insist the two agree.
        checked = sum(1 for _ in iter_pinned(catalog))
        present = count_hashes(catalog)
        if checked != present:
            print(
                f"ERROR  catalogue holds {present} sha256 value(s) but only {checked} "
                "were checked — an entry's shape is inconsistent",
                file=sys.stderr,
            )
            return 1
        print(f"OK  {checked} pinned file(s) match upstream")
        return 0

    if errors:
        # Refuse a partial rewrite: a catalogue half-updated from a flaky network is
        # worse than one that is merely stale.
        print("Refusing to update: some files could not be checked.", file=sys.stderr)
        return 1
    if drift:
        catalog["generated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        CATALOG.write_text(json.dumps(catalog, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"Updated {len(drift)} value(s) in {CATALOG.name}")
    else:
        print("Nothing to update")
    return 0


if __name__ == "__main__":
    sys.exit(main())
