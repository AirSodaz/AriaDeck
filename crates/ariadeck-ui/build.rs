fn main() {
    println!("cargo:rerun-if-env-changed=ARIADECK_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=ARIADECK_BUILD_NUMBER");

    let channel = std::env::var("ARIADECK_BUILD_CHANNEL").unwrap_or_else(|_| "dev".into());
    let build_number = std::env::var("ARIADECK_BUILD_NUMBER").unwrap_or_default();

    println!("cargo:rustc-env=ARIADECK_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=ARIADECK_BUILD_NUMBER={build_number}");
}
