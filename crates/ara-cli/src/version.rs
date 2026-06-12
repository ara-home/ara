/// The Ara package manager version, injected at build time.
///
/// Set the `ARA_VERSION` environment variable at build time to override
/// the version from `Cargo.toml`. This is used by cargo-dist and CI
/// pipelines to inject the correct release version.
pub const VERSION: &str = env!("ARA_VERSION");
