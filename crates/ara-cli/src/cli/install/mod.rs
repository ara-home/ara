pub(crate) mod cmd_add;
pub(crate) mod cmd_install_in;
pub(crate) mod disk_ops;
pub(crate) mod lockfile;
pub(crate) mod resolve;
pub(crate) mod transitive;
pub(crate) mod workspace;

// Re-exports: keep the same path as before (install::cmd_install, install::cmd_install_specs)
pub(crate) use cmd_add::cmd_install_specs;
pub(crate) use cmd_install_in::cmd_install;
// Benchmarks use these from outside the crate
pub use disk_ops::{extract_tarball, hardlink_dir, install_bin_links};
