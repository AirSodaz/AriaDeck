#!/usr/bin/env python3
"""Generate updater.json for AriaDeck release following Tauri v2 updater schema."""

import argparse
from datetime import datetime, timezone
import json
import os
import re
import sys
from pathlib import Path


def get_workspace_version(root: Path) -> str:
    cargo_toml = root / "Cargo.toml"
    if not cargo_toml.exists():
        return "0.0.0"
    content = cargo_toml.read_text(encoding="utf-8")
    m = re.search(r'\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"', content)
    if m:
        return m.group(1)
    return "0.0.0"


def find_file(dist: Path, pattern: str) -> Path | None:
    for p in dist.glob(pattern):
        if p.is_file():
            return p
    return None


def read_signature(sig_file: Path) -> str:
    return sig_file.read_text(encoding="utf-8").strip()


def main():
    parser = argparse.ArgumentParser(description="Generate updater.json for releases")
    parser.add_argument("--dist-dir", default="dist", help="Directory with release artifacts")
    parser.add_argument("--version", default=None, help="Application version (e.g. 0.2.0)")
    parser.add_argument("--tag", default=None, help="Release tag name (e.g. v0.2.0)")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "AirSodaz/AriaDeck"))
    parser.add_argument("--notes", default=None, help="Release notes text or file path")
    parser.add_argument("--output", default=None, help="Output path for updater.json")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent
    dist = (root / args.dist_dir).resolve() if not Path(args.dist_dir).is_absolute() else Path(args.dist_dir)
    version = args.version or get_workspace_version(root)
    tag = args.tag or f"v{version}"
    output = Path(args.output) if args.output else dist / "updater.json"

    notes = ""
    if args.notes:
        notes_path = Path(args.notes)
        if notes_path.is_file():
            notes = notes_path.read_text(encoding="utf-8").strip()
        else:
            notes = args.notes.strip()
    if not notes:
        notes = f"AriaDeck {version} release."

    pub_date = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    # Platform key mapping to file search pattern
    platform_patterns = {
        "darwin-aarch64": "*aarch64.app.tar.gz",
        "darwin-x86_64": "*x64.app.tar.gz",
        "linux-x86_64": "*amd64.AppImage",
        "windows-x86_64": "*x64-setup.exe",
        "windows-aarch64": "*arm64-setup.exe",
    }

    platforms = {}
    for platform_key, pattern in platform_patterns.items():
        matched_file = find_file(dist, pattern)
        if not matched_file:
            print(f"Warning: No artifact found for {platform_key} matching {pattern}", file=sys.stderr)
            continue
        sig_file = dist / f"{matched_file.name}.sig"
        if not sig_file.is_file():
            print(f"Warning: Signature file missing for {matched_file.name}: {sig_file}", file=sys.stderr)
            continue

        signature = read_signature(sig_file)
        url = f"https://github.com/{args.repo}/releases/download/{tag}/{matched_file.name}"
        platforms[platform_key] = {
            "signature": signature,
            "url": url,
        }

    manifest = {
        "version": version,
        "notes": notes,
        "pub_date": pub_date,
        "platforms": platforms,
    }

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Wrote updater manifest with {len(platforms)} platforms to {output}")


if __name__ == "__main__":
    main()
