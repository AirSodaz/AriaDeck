# Signed Cross-Platform Builds Design

## Intent

Build Windows, macOS, and Linux packages on pushes to `main` and attach cryptographic signatures so users can verify that packages came from AriaDeck and were not changed. Use the same verification chain for tagged releases and the existing Windows in-app installer download.

## Goals and constraints

- Add macOS arm64/x86_64 DMGs and Linux x86_64 AppImage/`.deb` to main CI artifacts, alongside the existing Windows portable zip and installer.
- Use a cross-platform update signature (Minisign), not OS trust signing or macOS notarization.
- Sign each final package file with Minisign using a private key stored as a GitHub Actions secret. Pin the corresponding public key in the app. Keep the SHA-256 manifest as an additional integrity check.
- Require the signing key in the trusted package/signing jobs so published CI and release package sets are never silently unsigned. PR verification jobs do not receive the key.
- Make the Windows in-app updater verify the installer package signature before trusting its SHA-256 manifest entry, then keep its existing hash verification.
- Mac/Linux package signatures support manual verification; adding Mac/Linux in-app update/install flows is out of scope.
- Keep existing optional Windows Authenticode signing behavior separate from update-package signatures.

## Approaches

- Sign each package file separately with Minisign: selected because it matches Tauri's update-payload signing model and lets clients authenticate the exact bytes they download; the existing SHA-256 manifest remains useful for bulk verification.
- Sign one SHA-256 manifest listing every package: compact and authenticates the package set, but differs from Tauri's payload-signing model and makes each client parse a shared index before it can authenticate its own download.
- Use GitHub keyless/Sigstore attestations: avoids a long-lived signing key but requires a different client verification and trust policy than the existing updater.

## Implementation

Extend main CI with native Windows, Ubuntu, and macOS package jobs. A final signing job collects their artifacts, signs each package, writes and verifies `SHA256SUMS.txt`, and publishes one combined artifact set with a `.minisig` sidecar per package. Apply the same signing step to tag releases and attach package signatures to the GitHub Release.

Update the Windows updater to require and verify the installer `.minisig` against the pinned public key before accepting the installer, then retain its existing SHA-256 check. Missing, invalid, or mismatched signatures fail closed. Document key setup, local/manual verification, artifact names, and the fact that Mac/Linux in-app updates are not included.

## Verification

Validate workflow YAML and conditions, check package jobs run only on trusted `main`/`master` pushes and tags, test signature creation and rejection of tampered/unsigned installers, run focused Rust update tests, and run workspace CI checks.
