# AriaDeck release packaging (RELEASE-001)

Windows-first distribution for AriaDeck. This document is the acceptance contract
for portable packages, the optional installer, data retention, signing hooks,
licenses, and upgrade/rollback expectations.

## Scope

| In scope | Out of scope (deferred) |
| --- | --- |
| Windows x64 portable zip | In-app auto-update product |
| Optional Inno Setup installer | Network download of official aria2 builds |
| Portable vs installed data dirs | macOS `.app` / Linux packages as primary artifacts |
| Uninstall keeps user data by default | SQLite history / browser associations |
| MIT app license + third-party notices | Production code-signing certificates in-repo |
| Settings/profile migration safety | Multi-platform store distribution |

## Artifacts

| Artifact | Layout |
| --- | --- |
| Portable | `dist/AriaDeck-<ver>-windows-x64-portable/` + `.zip` |
| Installer | `dist/AriaDeck-<ver>-windows-x64-setup.exe` (Inno Setup) |

Portable folder contents:

- `ariadeck-desktop.exe`
- `ariadeck.portable` (marker → data under `./data`)
- `LICENSE`, `THIRD_PARTY_NOTICES.md`, `README-portable.txt`

Installer contents: executable + licenses only (**no** portable marker).

Managed aria2 is **not** bundled. Users import/link a core in Settings → Engine
or connect with `ARIADECK_RPC_URL`.

## Data directory resolution

Order implemented in `default_data_dir` / `resolve_data_dir`:

1. `ARIADECK_DATA_DIR` when set  
2. `<exe_dir>/data` when `<exe_dir>/ariadeck.portable` exists  
3. `%LOCALAPPDATA%\AriaDeck` (Windows)  
4. `$XDG_DATA_HOME/ariadeck` or `~/.local/share/ariadeck`  
5. `./.ariadeck` fallback  

Typical files under the data dir:

- `settings.json` — versioned app prefs (schema migrations)  
- `window.json` — window geometry  
- `profiles.json` — engine profile catalog  
- `cores/` — managed aria2 core registry  
- `downloads/` — default download directory (unless overridden)  

## Uninstall / data retention

| Mode | Removing the app | User data |
| --- | --- | --- |
| Installer | Uninstall removes program files + shortcuts | `%LOCALAPPDATA%\AriaDeck` **kept** unless the uninstall checkbox is enabled |
| Portable | Delete the folder | `./data` goes with the folder |

## Version & binary metadata

Workspace version in root `Cargo.toml` (`workspace.package.version`) is the
release version. About UI uses `CARGO_PKG_VERSION`. Windows resources embed
ProductName / FileDescription / FileVersion via `apps/ariadeck-desktop/build.rs`.

## Packaging commands

```powershell
# Refresh third-party license summary (optional, before a release tag)
python scripts/gen_third_party_notices.py

# Portable package (+ zip)
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1

# Optional Authenticode sign (requires cert tooling)
$env:ARIADECK_SIGN_CERT_THUMBPRINT = "<thumbprint>"
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1 -Sign

# Optional installer (Inno Setup 6+ with ISCC on PATH)
iscc /DMyAppVersion=0.1.0 packaging\windows\AriaDeck.iss
```

## Code signing

Signing is optional and env-driven:

| Variable | Purpose |
| --- | --- |
| `ARIADECK_SIGN_TOOL` | Path to `signtool.exe` (else `signtool` on PATH) |
| `ARIADECK_SIGN_CERT_THUMBPRINT` | Certificate thumbprint in the store |
| `ARIADECK_SIGN_PFX` / `ARIADECK_SIGN_PFX_PASSWORD` | PFX-based signing |
| `ARIADECK_SIGN_DESCRIPTION` | `/d` description (default `AriaDeck`) |

Unsigned builds may trigger SmartScreen. No certificates live in the repository.

## Licenses

- Application: MIT (`LICENSE`)
- Dependency summary: `THIRD_PARTY_NOTICES.md` (from `cargo metadata`)
- UI framework: GPUI is Apache-2.0

Regenerate notices when dependencies change significantly:

```sh
python scripts/gen_third_party_notices.py
```

## Upgrade / rollback (app)

- **Upgrade:** install over previous version or replace the portable exe; settings
  migrations run on load (`ariadeck-settings` v1…current).
- **Downgrade:** newer `schema_version` fails closed without replacing the file
  (`UnsupportedSchemaVersion`); recover from backup or reinstall a matching app.
- **Managed aria2 cores:** activate/rollback via `CoreStore` (last-working);
  not an application auto-update channel.
- **Residual:** no in-app auto-update productization.

## Acceptance matrix

| Scenario | Guard |
| --- | --- |
| 10k-class app already shipped | PERF-001 (prior) |
| Portable data isolation | `resolve_data_dir_*` tests + marker file |
| Installed data path | LocalAppData without marker |
| Settings v1–v8 migrate | settings migration tests / matrix |
| Future schema rejected | `future_schema_is_rejected_*` |
| Uninstall keeps data | Inno default + docs checklist |
| Licenses in package | staged by portable script |
| Signing | optional script hook |

## Manual checklist

1. `cargo fmt --all --check`  
2. `cargo test --workspace`  
3. `cargo clippy --workspace --all-targets -- -D warnings`  
4. Build portable zip; launch with marker → confirm `./data` created  
5. Launch without marker → confirm `%LOCALAPPDATA%\AriaDeck`  
6. If installer built: uninstall without data checkbox → data remains  
7. Optional: sign and verify with `signtool verify /pa`
