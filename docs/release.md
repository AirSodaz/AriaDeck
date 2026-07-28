# AriaDeck release (Windows)

Acceptance contract for portable packages, optional installer, data dirs, signing, licenses.  
**Roadmap residual (signing, multi-OS):** [`roadmap.md`](roadmap.md) Phase A/E.

## Scope

| In | Out (deferred) |
| --- | --- |
| Windows x64 portable zip | In-app auto-update product |
| Optional Inno Setup installer | Network download of official aria2 |
| Portable vs installed data dirs | macOS/Linux as primary artifacts |
| Uninstall keeps user data by default | Store distribution |
| MIT + third-party notices | Production certs in-repo |

## Artifacts

| Artifact | Layout |
| --- | --- |
| Portable | `dist/AriaDeck-<ver>-windows-x64-portable/` + `.zip` |
| Installer | `dist/AriaDeck-<ver>-windows-x64-setup.exe` |
| Checksums | `dist/SHA256SUMS.txt` (tag builds only; `sha256sum -c` format, LF, no BOM) |

Portable: `ariadeck-desktop.exe`, `ariadeck.portable`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `README-portable.txt`.  
Installer: exe + licenses (**no** portable marker). **No** bundled aria2—import core or `ARIADECK_RPC_URL`.

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

## GitHub Actions

| Workflow | When | What |
| --- | --- | --- |
| `.github/workflows/ci.yml` → job `Windows packages (portable + installer)` | Push to `main` / `master` (after `verify`) | Notices → portable → `choco install innosetup` → installer `-SkipStage` → drop staging dir → assert zip + setup.exe → artifact `ariadeck-windows-x64` (zip + setup only) |
| `.github/workflows/release.yml` | Tag `v*` | Assert tag == `workspace.package.version` (fails before the build) → same packaging (optional Authenticode via secrets) → `SHA256SUMS.txt` → artifact + GitHub Release attaching `dist/*.zip`, `dist/*-setup.exe`, `dist/SHA256SUMS.txt`, with a fixed install/SmartScreen body plus auto-generated commit notes |

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

- Both packages ship `ariadeck-bridge.exe`; it is inert until registered.
- Installer offers an opt-in task, defaulted off, **only** when built with a valid
  `ARIADECK_EXTENSION_ID` (32 chars, `a`–`p`). Without it the task is omitted —
  registering with an empty `allowed_origins` would accept any extension.
- Portable: register by hand with `ariadeck-bridge.exe --register --extension-id <ID>`,
  and re-run after moving the folder (the registration records the current path).
- Uninstall always runs `--unregister`, removing both browser keys and the manifest.

## Upgrade / rollback

- Upgrade: overwrite portable or reinstall; settings migrate on load.
- Downgrade: newer `schema_version` fails closed (`UnsupportedSchemaVersion`).
- Cores: `CoreStore` activate/rollback—not app auto-update.

## Acceptance

| Scenario | Guard |
| --- | --- |
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
