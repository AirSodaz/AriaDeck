//! Accessibility helpers for ACCESS-001 (reduced motion, motion preference).

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide override for tests; `None` means "use platform default".
static REDUCED_MOTION_OVERRIDE: AtomicBool = AtomicBool::new(false);
static HAS_OVERRIDE: AtomicBool = AtomicBool::new(false);

/// Whether the user prefers reduced motion.
///
/// Order of precedence:
/// 1. Test/runtime override via [`set_prefers_reduced_motion_override`]
/// 2. `ARIADECK_REDUCED_MOTION=1|true|yes` environment variable
/// 3. Default: no preference (`false`)
///
/// Platform SPI queries are intentionally avoided here because `ariadeck-ui`
/// forbids `unsafe`. Desktop can set the override at bootstrap later if needed.
#[must_use]
pub fn prefers_reduced_motion() -> bool {
    if HAS_OVERRIDE.load(Ordering::Relaxed) {
        return REDUCED_MOTION_OVERRIDE.load(Ordering::Relaxed);
    }
    if let Ok(value) = std::env::var("ARIADECK_REDUCED_MOTION") {
        let value = value.trim();
        if value.eq_ignore_ascii_case("1")
            || value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("yes")
        {
            return true;
        }
        if value.eq_ignore_ascii_case("0")
            || value.eq_ignore_ascii_case("false")
            || value.eq_ignore_ascii_case("no")
        {
            return false;
        }
    }
    false
}

/// Force reduced-motion preference for tests. Pass `None` to clear the override.
pub fn set_prefers_reduced_motion_override(value: Option<bool>) {
    match value {
        Some(flag) => {
            REDUCED_MOTION_OVERRIDE.store(flag, Ordering::Relaxed);
            HAS_OVERRIDE.store(true, Ordering::Relaxed);
        }
        None => {
            HAS_OVERRIDE.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_controls_prefers_reduced_motion() {
        set_prefers_reduced_motion_override(Some(true));
        assert!(prefers_reduced_motion());
        set_prefers_reduced_motion_override(Some(false));
        assert!(!prefers_reduced_motion());
        set_prefers_reduced_motion_override(None);
    }
}
