//! Native messaging host registration (D-045 §2).
//!
//! The browser finds the host through a manifest file named by a registry key.
//! Both are written here rather than by the installer script so the logic is
//! testable, and so a portable install that moved directories can re-register
//! itself without a reinstall.
//!
//! Registration is an explicit action either way: the installer runs it only when
//! the user opts in, and it refuses to run at all without a pinned extension ID —
//! a manifest with an empty `allowed_origins` would let *any* extension launch the
//! host, which is the trust boundary the whole design rests on.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;

/// Chrome native-messaging host name. Must match the extension's `connectNative`
/// argument and the host-name grammar (`[a-z0-9._]`).
pub const HOST_NAME: &str = "com.ariadeck.bridge";

/// File name of the generated manifest, written next to the host executable.
pub const MANIFEST_FILE: &str = "com.ariadeck.bridge.json";

/// Browsers to register with. Chrome and Edge share the native messaging
/// protocol and differ only in the key path.
const BROWSER_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
];

/// Extension ID baked in at build time, when the build knows it.
///
/// Not a fallback for convenience: it is how a release build carries the pinned
/// ID so the installer does not have to pass a secret-looking string around.
const BUILT_IN_EXTENSION_ID: Option<&str> = option_env!("ARIADECK_EXTENSION_ID");

#[derive(Serialize)]
struct HostManifest {
    name: &'static str,
    description: &'static str,
    path: String,
    #[serde(rename = "type")]
    kind: &'static str,
    allowed_origins: Vec<String>,
}

/// Resolve the extension ID to pin, in order: explicit argument, environment,
/// build-time constant. Absent or malformed means no registration.
pub fn resolve_extension_id(argument: Option<&str>) -> io::Result<String> {
    let candidate = argument
        .map(str::to_owned)
        .or_else(|| std::env::var("ARIADECK_EXTENSION_ID").ok())
        .or_else(|| BUILT_IN_EXTENSION_ID.map(str::to_owned));
    let Some(candidate) = candidate else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no extension ID: pass --extension-id, or set ARIADECK_EXTENSION_ID. \
             Registering without one would let any extension launch the host.",
        ));
    };
    validate_extension_id(&candidate)?;
    Ok(candidate)
}

/// A Chrome extension ID is exactly 32 characters from `a`..=`p`.
///
/// Checked strictly because this string is the whole allow-list: a typo that
/// still parsed would pin the host to an extension nobody controls.
fn validate_extension_id(id: &str) -> io::Result<()> {
    if id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() && byte <= b'p')
    {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "extension ID must be 32 characters in the range a-p",
    ))
}

/// Directory holding the host executable; the manifest is written beside it.
fn install_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot determine the host executable directory",
        )
    })
}

fn manifest_json(exe: &Path, extension_id: &str) -> io::Result<Vec<u8>> {
    let manifest = HostManifest {
        name: HOST_NAME,
        description: "AriaDeck browser bridge",
        path: exe.to_string_lossy().into_owned(),
        kind: "stdio",
        allowed_origins: vec![format!("chrome-extension://{extension_id}/")],
    };
    serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)
}

/// Write the manifest and point Chrome and Edge at it.
pub fn register(extension_id: &str) -> io::Result<PathBuf> {
    let dir = install_dir()?;
    let manifest_path = dir.join(MANIFEST_FILE);
    let json = manifest_json(&std::env::current_exe()?, extension_id)?;
    std::fs::write(&manifest_path, json)?;
    set_browser_keys(&BROWSER_KEYS, &manifest_path)?;
    Ok(manifest_path)
}

/// Remove both registry keys and the generated manifest.
///
/// Best-effort per step: a half-registered install must still end up fully
/// unregistered, so a missing key is not an error.
pub fn unregister() -> io::Result<()> {
    let mut first_error = clear_browser_keys(&BROWSER_KEYS).err();
    if let Ok(dir) = install_dir() {
        let manifest_path = dir.join(MANIFEST_FILE);
        if let Err(error) = std::fs::remove_file(&manifest_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            first_error = first_error.or(Some(error));
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

// `roots` is a parameter rather than a constant read so the round-trip can be
// exercised against throwaway keys instead of the ones a real browser reads.
#[cfg(windows)]
fn set_browser_keys(roots: &[&str], manifest_path: &Path) -> io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let value = manifest_path.to_string_lossy().into_owned();
    for root in roots {
        let (key, _) = hkcu.create_subkey(format!(r"{root}\{HOST_NAME}"))?;
        // The default value is the manifest path; that is the whole contract.
        key.set_value("", &value)?;
    }
    Ok(())
}

#[cfg(windows)]
fn clear_browser_keys(roots: &[&str]) -> io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut first_error = None;
    for root in roots {
        // Only AriaDeck's own subkey is removed; NativeMessagingHosts is shared
        // with every other host the user has installed.
        if let Err(error) = hkcu.delete_subkey_all(format!(r"{root}\{HOST_NAME}"))
            && error.kind() != io::ErrorKind::NotFound
        {
            first_error = first_error.or(Some(error));
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(not(windows))]
fn set_browser_keys(_roots: &[&str], _manifest_path: &Path) -> io::Result<()> {
    Err(unsupported_platform())
}

#[cfg(not(windows))]
fn clear_browser_keys(_roots: &[&str]) -> io::Result<()> {
    Err(unsupported_platform())
}

#[cfg(not(windows))]
fn unsupported_platform() -> io::Error {
    // Chrome on macOS and Linux uses per-user manifest *directories* rather than
    // a registry key. Wiring those up belongs with the platform ports, which are
    // deferred alongside the Firefox port (browser-bridge.md §9).
    io::Error::new(
        io::ErrorKind::Unsupported,
        "host registration is implemented for Windows only",
    )
}

/// Print what registration did, for the installer log and manual runs. Never
/// echoes anything but the manifest path and the pinned ID.
pub fn report_registered(manifest_path: &Path, extension_id: &str) {
    let _ = writeln!(
        io::stdout(),
        "registered {HOST_NAME}\n  manifest:  {}\n  extension: {extension_id}",
        manifest_path.display()
    );
}

/// Removal is idempotent, so it cannot report what it removed without racing.
/// It still has to say something: run by hand, silence is indistinguishable from
/// having done nothing. The installer runs it hidden and ignores this.
pub fn report_unregistered() {
    let _ = writeln!(io::stdout(), "unregistered {HOST_NAME}");
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn extension_ids_outside_the_chrome_alphabet_are_refused() {
        validate_extension_id(VALID_ID).expect("a canonical ID is accepted");
        for bad in [
            "",
            "tooshort",
            "abcdefghijklmnopabcdefghijklmno",   // 31
            "abcdefghijklmnopabcdefghijklmnopq", // 33
            "abcdefghijklmnopabcdefghijklmnoq",  // q is out of range
            "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP",  // upper case
            "abcdefghijklmnopabcdefghijklmno1",  // digit
            "abcdefghijklmnop-bcdefghijklmnop",  // punctuation
        ] {
            assert!(
                validate_extension_id(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
    }

    /// Without an ID there is no registration at all — a manifest with an empty
    /// allow-list would let any extension launch the host.
    #[test]
    fn registration_refuses_to_proceed_without_a_pinned_extension_id() {
        let resolved = resolve_extension_id(None);
        // The environment or a build-time constant may legitimately supply one;
        // what must never happen is an empty or malformed value being accepted.
        match resolved {
            Ok(id) => validate_extension_id(&id).expect("a resolved ID is always valid"),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::InvalidInput),
        }
        assert_eq!(
            resolve_extension_id(Some(""))
                .expect_err("an empty argument is not an ID")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    /// Exercises the real registry code against throwaway keys under HKCU, so a
    /// test run can never disturb what a browser actually reads.
    #[cfg(windows)]
    #[test]
    fn browser_keys_round_trip_and_uninstall_removes_only_our_subkey() {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        // One key, one level deep, so the cleanup below leaves nothing behind —
        // not even an empty parent.
        let base = format!(
            r"Software\AriaDeck-bridge-register-test-{}",
            std::process::id()
        );
        let roots = [format!(r"{base}\Chrome"), format!(r"{base}\Edge")];
        let roots: Vec<&str> = roots.iter().map(String::as_str).collect();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // A sibling host that must survive our uninstall.
        let (sibling, _) = hkcu
            .create_subkey(format!(r"{}\com.someone.else", roots[0]))
            .expect("sibling host key");
        sibling
            .set_value("", &"C:\\other\\manifest.json".to_owned())
            .expect("sibling value");

        let manifest_path = Path::new(r"C:\Program Files\AriaDeck\com.ariadeck.bridge.json");
        set_browser_keys(&roots, manifest_path).expect("keys are written");
        for root in &roots {
            let key = hkcu
                .open_subkey(format!(r"{root}\{HOST_NAME}"))
                .expect("host key exists");
            let value: String = key
                .get_value("")
                .expect("default value is the manifest path");
            assert_eq!(value, manifest_path.to_string_lossy());
        }

        clear_browser_keys(&roots).expect("keys are removed");
        for root in &roots {
            assert!(
                hkcu.open_subkey(format!(r"{root}\{HOST_NAME}")).is_err(),
                "uninstall must remove the host key"
            );
        }
        assert!(
            hkcu.open_subkey(format!(r"{}\com.someone.else", roots[0]))
                .is_ok(),
            "uninstall must not touch other hosts sharing the parent key"
        );
        // Unregistering twice is what a repeated uninstall does.
        clear_browser_keys(&roots).expect("a second unregister is not an error");

        hkcu.delete_subkey_all(&base)
            .expect("the test leaves no registry residue");
        assert!(hkcu.open_subkey(&base).is_err());
    }

    #[test]
    fn manifest_pins_the_extension_and_declares_stdio() {
        let exe = Path::new(r"C:\Program Files\AriaDeck\ariadeck-bridge.exe");
        let json = manifest_json(exe, VALID_ID).expect("manifest serializes");
        let value: serde_json::Value =
            serde_json::from_slice(&json).expect("manifest is valid JSON");

        assert_eq!(value["name"], HOST_NAME);
        assert_eq!(value["type"], "stdio");
        assert_eq!(
            value["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{VALID_ID}/")]),
            "exactly one origin, pinned"
        );
        // Backslashes must survive as a usable path, not as broken escapes.
        assert_eq!(value["path"], exe.to_string_lossy().into_owned());
    }
}
