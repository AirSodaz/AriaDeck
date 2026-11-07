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
key_file="$(mktemp)"
public_key_file="$(mktemp)"
trap 'rm -f "$key_file" "$public_key_file"' EXIT
printf '%s\n' "$ARIADECK_UPDATE_SIGNING_KEY" > "$key_file"
printf '%s\n' "$ARIADECK_UPDATE_PUBLIC_KEY" > "$public_key_file"

for artifact in "$@"; do
  [[ -f "$artifact" ]] || { echo "artifact not found: $artifact" >&2; exit 1; }
  minisign -S -W -s "$key_file" -m "$artifact" -x "$artifact.minisig" -t "AriaDeck update package"
  minisign -V -m "$artifact" -x "$artifact.minisig" -p "$public_key_file"
  echo "Signed $artifact"
done
