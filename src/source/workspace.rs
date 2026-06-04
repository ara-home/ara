// WorkspaceSource is an alias for LocalSource — both create local tarballs
// from a directory. The distinction is semantic (workspace member vs path dep).
pub use super::local::LocalSource as WorkspaceSource;
