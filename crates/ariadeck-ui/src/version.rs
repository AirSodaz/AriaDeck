pub(crate) fn app_version() -> String {
    format_app_version(
        env!("CARGO_PKG_VERSION"),
        env!("ARIADECK_BUILD_CHANNEL"),
        env!("ARIADECK_BUILD_NUMBER"),
    )
}

fn format_app_version(version: &str, channel: &str, build_number: &str) -> String {
    match channel {
        "release" => format!("{version}-release"),
        "ci" if !build_number.trim().is_empty() => {
            format!("{version}-ci.{}", build_number.trim())
        }
        "ci" => format!("{version}-ci"),
        _ => format!("{version}-dev"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_app_version;

    #[test]
    fn formats_each_build_channel() {
        assert_eq!(
            format_app_version("0.2.0", "release", "99"),
            "0.2.0-release"
        );
        assert_eq!(format_app_version("0.2.0", "ci", "123"), "0.2.0-ci.123");
        assert_eq!(format_app_version("0.2.0", "ci", " "), "0.2.0-ci");
        assert_eq!(format_app_version("0.2.0", "dev", "123"), "0.2.0-dev");
        assert_eq!(format_app_version("0.2.0", "unknown", "123"), "0.2.0-dev");
    }
}
