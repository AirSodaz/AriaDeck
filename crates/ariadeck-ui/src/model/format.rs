/// Full message catalogs remain deferred; this only localizes *number shape*
/// (decimal separator) while keeping stable IEC unit labels for tests and UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatOptions {
    /// Character used between integer and fractional parts.
    pub decimal_separator: char,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self::en()
    }
}

impl FormatOptions {
    /// English / invariant formatting used by default and unit tests.
    #[must_use]
    pub const fn en() -> Self {
        Self {
            decimal_separator: '.',
        }
    }

    /// Prefer a comma decimal separator (common in many European locales).
    #[must_use]
    pub const fn comma_decimal() -> Self {
        Self {
            decimal_separator: ',',
        }
    }

    /// Best-effort OS locale hint without a full i18n stack.
    ///
    /// Uses `LC_ALL` / `LC_NUMERIC` / `LANG` when present; falls back to [`Self::en`].
    #[must_use]
    pub fn from_env() -> Self {
        let locale = ["LC_ALL", "LC_NUMERIC", "LANG"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok())
            .unwrap_or_default();
        let locale = locale.to_ascii_lowercase();
        // Rough heuristic: many comma-decimal locales use de/fr/es/it/pt/ru/nl/…
        let comma_prefixes = [
            "de", "fr", "es", "it", "pt", "ru", "nl", "pl", "tr", "cs", "sk", "hu", "ro", "da",
            "sv", "nb", "nn", "fi", "el", "uk", "bg", "hr", "sl", "sr", "id", "vi",
        ];
        if comma_prefixes
            .iter()
            .any(|prefix| locale.starts_with(prefix) || locale.contains(&format!("_{prefix}")))
        {
            Self::comma_decimal()
        } else {
            Self::en()
        }
    }

    fn format_fixed(self, value: f64, precision: usize) -> String {
        let raw = match precision {
            0 => format!("{value:.0}"),
            1 => format!("{value:.1}"),
            2 => format!("{value:.2}"),
            _ => format!("{value:.1}"),
        };
        if self.decimal_separator == '.' {
            raw
        } else {
            raw.replace('.', &self.decimal_separator.to_string())
        }
    }
}

thread_local! {
    static ACTIVE_FORMAT_OPTIONS: std::cell::Cell<FormatOptions> =
        const { std::cell::Cell::new(FormatOptions::en()) };
}

/// Install process-local format options used by the convenience formatters.
pub fn set_active_format_options(options: FormatOptions) {
    ACTIVE_FORMAT_OPTIONS.with(|cell| cell.set(options));
}

/// Current process-local format options (defaults to English invariant).
#[must_use]
pub fn active_format_options() -> FormatOptions {
    ACTIVE_FORMAT_OPTIONS.with(|cell| cell.get())
}

#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    format_bytes_with(bytes, active_format_options())
}

#[must_use]
pub fn format_bytes_with(bytes: u64, options: FormatOptions) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value < 10.0 {
        format!("{} {}", options.format_fixed(value, 1), UNITS[unit])
    } else {
        format!("{} {}", options.format_fixed(value, 0), UNITS[unit])
    }
}

#[must_use]
pub fn format_rate(bytes_per_second: u64) -> String {
    format_rate_with(bytes_per_second, active_format_options())
}

#[must_use]
pub fn format_rate_with(bytes_per_second: u64, options: FormatOptions) -> String {
    format!("{}/s", format_bytes_with(bytes_per_second, options))
}

#[must_use]
pub fn format_eta(seconds: Option<u64>) -> String {
    format_eta_with(seconds, active_format_options())
}

#[must_use]
pub fn format_eta_with(seconds: Option<u64>, _options: FormatOptions) -> String {
    let Some(seconds) = seconds else {
        return "—".into();
    };
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3_600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
}

#[must_use]
pub fn format_percent(basis_points: Option<u16>) -> String {
    format_percent_with(basis_points, active_format_options())
}

#[must_use]
pub fn format_percent_with(basis_points: Option<u16>, options: FormatOptions) -> String {
    basis_points.map_or_else(
        || "—".into(),
        |value| {
            let percent = f64::from(value) / 100.0;
            format!("{}%", options.format_fixed(percent, 1))
        },
    )
}

#[must_use]
pub fn format_share_ratio(milli: Option<u64>) -> String {
    format_share_ratio_with(milli, active_format_options())
}

#[must_use]
pub fn format_share_ratio_with(milli: Option<u64>, options: FormatOptions) -> String {
    milli.map_or_else(
        || "—".into(),
        |value| {
            let hundredths = value.saturating_add(5) / 10;
            let whole = hundredths / 100;
            let frac = hundredths % 100;
            format!("{whole}{}{:02}", options.decimal_separator, frac)
        },
    )
}

/// Relative duration for activity / history timestamps (ACCESS date localization surface).
#[must_use]
pub fn format_relative_time(seconds_ago: u64) -> String {
    format_relative_time_with(seconds_ago, active_format_options())
}

#[must_use]
pub fn format_relative_time_with(seconds_ago: u64, _options: FormatOptions) -> String {
    if seconds_ago < 60 {
        return "just now".into();
    }
    if seconds_ago < 3_600 {
        let minutes = seconds_ago / 60;
        return if minutes == 1 {
            "1 minute ago".into()
        } else {
            format!("{minutes} minutes ago")
        };
    }
    if seconds_ago < 86_400 {
        let hours = seconds_ago / 3_600;
        return if hours == 1 {
            "1 hour ago".into()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = seconds_ago / 86_400;
    if days == 1 {
        "1 day ago".into()
    } else {
        format!("{days} days ago")
    }
}
