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

## Commands

```powershell
python scripts/gen_third_party_notices.py
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1

# Optional sign
$env:ARIADECK_SIGN_CERT_THUMBPRINT = "<thumbprint>"
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1 -Sign

# Installer (Inno Setup 6+)
powershell -ExecutionPolicy Bypass -File scripts/package-windows-installer.ps1 -SkipBuild
```

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
11. Optional: `signtool verify /pa`
