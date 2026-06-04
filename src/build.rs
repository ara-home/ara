fn main() {
    let version = std::env::var("ARA_VERSION").unwrap_or_else(|_| {
        std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string())
    });
    println!("cargo:rustc-env=ARA_VERSION={version}");
    println!("cargo:rerun-if-env-changed=ARA_VERSION");
}
