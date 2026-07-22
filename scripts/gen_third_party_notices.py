#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.md from cargo metadata (RELEASE-001)."""

from __future__ import annotations

import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    out = root / "THIRD_PARTY_NOTICES.md"

    proc = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if proc.returncode != 0 or not proc.stdout:
        sys.stderr.buffer.write(proc.stderr or b"cargo metadata failed\n")
        return proc.returncode or 1

    meta = json.loads(proc.stdout.decode("utf-8", errors="replace"))
    workspace = set(meta.get("workspace_members", []))
    by_license: dict[str, set[str]] = defaultdict(set)
    for package in meta["packages"]:
        if package["id"] in workspace or package.get("source") is None:
            continue
        license_id = package.get("license") or "UNKNOWN"
        by_license[license_id].add(f"{package['name']} {package['version']}")

    lines = [
        "# Third-Party Notices",
        "",
        "AriaDeck includes the following third-party Rust crates (direct and transitive).",
        "This summary was generated from `cargo metadata` for RELEASE-001 packaging.",
        "Upstream SPDX license identifiers are listed as reported by each crate.",
        "Full license texts are available from crates.io or the package source repository.",
        "",
        "AriaDeck itself is MIT-licensed (see `LICENSE`).",
        "GPUI is Apache-2.0 (dependency of the desktop UI).",
        "",
    ]
    for license_id in sorted(by_license.keys(), key=str.lower):
        lines.append(f"## {license_id}")
        lines.append("")
        for name in sorted(by_license[license_id]):
            lines.append(f"- {name}")
        lines.append("")

    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    crate_count = sum(len(v) for v in by_license.values())
    print(f"wrote {out} ({len(by_license)} license groups, {crate_count} crates)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
