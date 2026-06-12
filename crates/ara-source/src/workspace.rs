// WorkspaceSource is an alias for LocalSource — both create local tarballs
// from a directory. The distinction is semantic (workspace member vs path dep).
pub use super::local::LocalSource as WorkspaceSource;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_source_alias() {
        let src = WorkspaceSource::new("/tmp".to_string());
        assert_eq!(src.path, "/tmp");
    }
}
