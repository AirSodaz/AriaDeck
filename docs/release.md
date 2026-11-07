# AriaDeck release (Windows + Linux)

Acceptance contract for portable packages, optional installer, data dirs, signing, licenses.  
**Roadmap residual (signing, multi-OS):** [`roadmap.md`](roadmap.md) Phase A/E.

## Scope

| In | Out (deferred) |
| --- | --- |
| Windows x64 portable zip + installer | macOS app bundle and notarization |
| Linux x86_64 AppImage + Debian package | ARM64, RPM, and Flatpak packages |
| Optional Inno Setup installer | Network download of official aria2 |
| Portable and installed data directories; uninstall keeps data by default | Store distribution |
| MIT + third-party notices | Production certs in-repo |

## Artifacts

| Artifact | Layout |
| --- | --- |
| Portable | `dist/AriaDeck-<ver>-windows-x64-portable/` + `.zip` |
| Installer | `dist/AriaDeck-<ver>-windows-x64-setup.exe` |
| Linux AppImage | `dist/AriaDeck-<ver>-x86_64.AppImage` |
| Debian package | `dist/ariadeck_<ver>_amd64.deb` |
| Checksums | `dist/SHA256SUMS.txt` (tag builds only; `sha256sum -c` format, LF) |

Portable: `ariadeck-desktop.exe`, `ariadeck.portable`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `README-portable.txt`.  
Installer: exe + licenses (**no** portable marker). **No** bundled aria2—import core or `ARIADECK_RPC_URL`.

Linux AppImage and `.deb`: desktop binary, launcher, icon, license, and third-party notices. The `.deb` declares runtime dependencies from the Ubuntu 22.04 build. AppImage bundles discoverable shared-library dependencies; GTK 3 and other desktop-system components remain host requirements. Linux native-messaging registration is not available yet, so `ariadeck-bridge` is not included.

## Data directory

Order (`default_data_dir` / `resolve_data_dir`):

1. `ARIADECK_DATA_DIR`
2. `<exe_dir>/data` if `ariadeck.portable` exists
3. `%LOCALAPPDATA%\AriaDeck` (Windows)
4. `$XDG_DATA_HOME/ariadeck` or `~/.local/share/ariadeck`
5. `./.ariadeck`

Typical files: `settings.json`, `window.json`, `profiles.json`, `cores/`, `downloads/`.

| Mode | App remove | User data |
| --- | --- | --- |
| Installer | Program files + shortcuts | LocalAppData **kept** unless uninstall checkbox |
| Portable | Delete folder | `./data` goes with it |

## Version

Root `Cargo.toml` `workspace.package.version` · About uses `CARGO_PKG_VERSION` · winres via `apps/ariadeck-desktop/build.rs`.  
Bump it **before** tagging: the release tag must be exactly `v<version>` or `release.yml` stops before building.

## Commands

Local build (mirrors CI packaging steps):

```powershell
python scripts/gen_third_party_notices.py
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1

# Optional sign
$env:ARIADECK_SIGN_CERT_THUMBPRINT = "<thumbprint>"
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1 -Sign

# Installer (Inno Setup 6+). -SkipStage reuses dist staging from the portable step (CI does this).
powershell -ExecutionPolicy Bypass -File scripts/package-windows-installer.ps1 -SkipStage
# Or restage without rebuild:
powershell -ExecutionPolicy Bypass -File scripts/package-windows-installer.ps1 -SkipBuild
```

Expected outputs under `dist/`:

- `AriaDeck-<ver>-windows-x64-portable/` + `.zip`
- `AriaDeck-<ver>-windows-x64-setup.exe`
- `AriaDeck-<ver>-x86_64.AppImage`
- `ariadeck_<ver>_amd64.deb`

Linux build and package verification (Ubuntu 22.04, x86_64):

```sh
python3 scripts/gen_third_party_notices.py
cargo build --locked -p ariadeck-desktop --release
bash scripts/package-linux.sh <version> /path/to/linuxdeploy-x86_64.AppImage
```

The package script verifies AppImage extraction, bundled binary dependencies, Debian metadata, and required package contents. `linuxdeploy` is pinned in the release workflow.

## GitHub Actions

| Workflow | When | What |
| --- | --- | --- |
| `.github/workflows/ci.yml` → job `Windows packages (portable + installer)` | Push to `main` / `master` (after `verify`) | Notices → portable → `choco install innosetup` → installer `-SkipStage` → drop staging dir → assert zip + setup.exe → artifact `ariadeck-windows-x64` (zip + setup only) |
| `.github/workflows/release.yml` | Tag `v*` | Windows and Ubuntu 22.04 x86_64 package jobs verify the version and upload artifacts; a publish job waits for both, checks all four packages, creates `SHA256SUMS.txt`, and attaches the Windows zip/setup, Linux AppImage/deb, and checksums to the GitHub Release |

PR / non-main branches run **verify only** (fmt/test/clippy/release build); they do not produce installers.

### Signing env

| Variable | Purpose |
| --- | --- |
| `ARIADECK_SIGN_TOOL` | `signtool.exe` path |
| `ARIADECK_SIGN_CERT_THUMBPRINT` | Store thumbprint |
| `ARIADECK_SIGN_PFX` / `ARIADECK_SIGN_PFX_PASSWORD` | PFX signing |
| `ARIADECK_SIGN_DESCRIPTION` | `/d` (default `AriaDeck`) |
| `ARIADECK_INNO_SETUP` | Optional full path to `ISCC.exe` |

Unsigned builds may hit SmartScreen. No certs in-repo.

## Licenses

- App: MIT (`LICENSE`)
- Deps: `THIRD_PARTY_NOTICES.md` (`python scripts/gen_third_party_notices.py`)
- GPUI: Apache-2.0

## Browser bridge (D-045)

- Both Windows packages ship `ariadeck-bridge.exe`; it is inert until registered.
- Installer offers an opt-in task, defaulted off, **only** when built with a valid
  `ARIADECK_EXTENSION_ID` (32 chars, `a`–`p`). Without it the task is omitted —
  registering with an empty `allowed_origins` would accept any extension.
- Portable: register by hand with `ariadeck-bridge.exe --register --extension-id <ID>`,
  and re-run after moving the folder (the registration records the current path).
- Uninstall always runs `--unregister`, removing both browser keys and the manifest.

## Upgrade / rollback

- Upgrade: overwrite portable or reinstall; settings migrate on load.
- Downgrade: newer `schema_version` fails closed (`UnsupportedSchemaVersion`).
- Cores: `CoreStore` activate/rollback, separately from application updates.

## Application updates

- Official Windows x64 release builds check GitHub Releases once at startup; About also has a manual check. Development/CI builds and other platforms do not contact the update feed.
- Portable builds only show the available version and release notes, with a link to the GitHub Release page. They never download or replace application files.
- Installed builds download the matching `*-windows-x64-setup.exe` only after the user chooses Download installer. The app verifies its SHA-256 against `SHA256SUMS.txt` from that release before offering Install and restart.
- The Inno installer upgrades the existing per-user installation; AriaDeck application data remains under `%LOCALAPPDATA%\AriaDeck`. Cancelling or failing the installer does not change that data.

## Acceptance

| Scenario | Guard |
| --- | --- |
| Linux AppImage | AppImage extraction, launcher/icon/license contents, and dependency checks in `scripts/package-linux.sh` |
| Linux Debian package | `dpkg-deb` metadata, runtime dependencies, launcher/icon/license contents in `scripts/package-linux.sh` |
| Release asset set | Publish job requires exactly four packages and hashes all four |
| Portable isolation | `resolve_data_dir_*` + marker |
| Installed path | LocalAppData without marker |
| Settings v1…current | migration tests |
| Future schema rejected | `future_schema_is_rejected_*` |
| Uninstall keeps data | Inno default |
| Licenses staged | portable script |
| File associations | Explicit default-unchecked Inno task; owned ProgIDs only |
| External metadata open | `--open-metadata` → preview/confirmation; running instance activated |
| Magnet protocol | Explicit default-unchecked Inno task; `--open-magnet` fills links without submission |
| Windows DPI awareness | GPUI `windows-manifest` is explicit; release EXE contains `PerMonitorV2` + `true/pm` |
| Reparse-point containment | Windows junction regression tests reject destination components and Trash task directories |
| Application update | Release-only feed; portable is notice/link only; installed setup must match same-release SHA-256 before launch |

## Manual checklist

1. `cargo fmt --all --check`
2. `cargo test --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. Portable zip + marker → `./data`
5. No marker → `%LOCALAPPDATA%\AriaDeck`
6. Installer uninstall without data checkbox → data remains
7. Installer association task defaults unchecked; opting in registers `.torrent`, `.metalink`, `.meta4`
8. Double-click while closed and while tray-hidden opens one preview without auto-submitting
9. Protocol task defaults unchecked; opting in registers `magnet:` and fills Add Download without submitting
10. Uninstall removes AriaDeck values without deleting shared extension or protocol keys
11. Launch with a managed local engine: no `aria2c` console window appears (spawn and probes use `CREATE_NO_WINDOW`)
12. Released assets match `SHA256SUMS.txt` (`sha256sum -c`, or `Get-FileHash -Algorithm SHA256`)
13. Optional: `signtool verify /pa`
14. Run `pwsh -File scripts/verify-windows-a3.ps1`; it tests real directory junctions, builds the release EXE, and verifies its embedded Per-Monitor V2 manifest. Windows CI repeats the automated portion after its release build.
15. At Windows 125% scale, run the script again with `-SkipBuild -ExpectedScale 125`, launch the printed EXE, and inspect the main task list, Add Download, task details, Settings, Profiles, and first-run core setup at the 960x620 minimum and maximized. Text must not clip or overlap; controls and modal actions must remain visible and usable.
16. Repeat step 15 at 150% with `-ExpectedScale 150`. When two monitors with different scales are available, drag the open window between them and confirm it redraws sharply without changing its logical minimum size or losing pointer alignment.
17. On a portable release build, confirm startup/manual checks only show version, notes, and the Release page link.
18. On an installed release build, confirm installer download requires user action, checksum failure prevents installation, and the verified installer is offered for explicit confirmation.
19. On Ubuntu 22.04 x86_64, extract the AppImage and confirm its launcher, icon, license, and desktop binary are present; launch it in a desktop session.
20. Install the .deb on Ubuntu 22.04 x86_64, confirm apt resolves its declared dependencies, launch AriaDeck, then remove the package and confirm user data remains under the XDG data directory.
