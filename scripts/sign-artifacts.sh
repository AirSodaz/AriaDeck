#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: sign-artifacts.sh ARTIFACT..." >&2
  exit 2
fi

command -v minisign >/dev/null || { echo "minisign is required" >&2; exit 1; }
[[ -n "${ARIADECK_UPDATE_SIGNING_KEY:-}" ]] || {
  echo "ARIADECK_UPDATE_SIGNING_KEY is required" >&2
  exit 1
}
[[ -n "${ARIADECK_UPDATE_PUBLIC_KEY:-}" ]] || {
  echo "ARIADECK_UPDATE_PUBLIC_KEY is required" >&2
  exit 1
}

umask 077
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

key_file="$work_dir/signing.key"
public_key_file="$work_dir/signing.pub"
printf '%s\n' "$ARIADECK_UPDATE_SIGNING_KEY" > "$key_file"
printf '%s\n' "$ARIADECK_UPDATE_PUBLIC_KEY" > "$public_key_file"

idx=0
for artifact in "$@"; do
  [[ -f "$artifact" ]] || { echo "artifact not found: $artifact" >&2; exit 1; }
  # macOS DMG is not signed with update signature (per matrix specification).
  if [[ "$artifact" == *.dmg ]]; then
    echo "Skipping signature for DMG: $artifact"
    continue
  fi
  idx=$((idx + 1))
  tmp_minisig="$work_dir/artifact-${idx}.minisig"
  verify_tmp="$work_dir/artifact-${idx}.verify"

  minisign -S -W -s "$key_file" -m "$artifact" -x "$tmp_minisig" -t "AriaDeck update package"
  minisign -V -m "$artifact" -x "$tmp_minisig" -p "$public_key_file"

  # Encode the Minisign signature file into a single-line Base64 string for .sig (Tauri updater style).
  base64 < "$tmp_minisig" | tr -d '\r\n' > "$artifact.sig"

  # Verify that the generated .sig decodes and validates cleanly.
  base64 -d < "$artifact.sig" > "$verify_tmp"
  minisign -V -m "$artifact" -x "$verify_tmp" -p "$public_key_file" >/dev/null

  echo "Signed $artifact -> $artifact.sig"
done
