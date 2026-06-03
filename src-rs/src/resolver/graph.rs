#![allow(dead_code)]

use crate::types::{SourceType, Version};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub name: String,
    pub source: SourceType,
    pub version: Version,
    pub package_hash: Option<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
}

impl Graph {
    #[must_use]
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    #[must_use]
    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Compute a hash for the graph. Currently a stub.
    #[must_use]
    pub fn compute_hash(&self) -> [u8; 32] {
        [0u8; 32]
    }

    /// Check for cycles. Currently a stub.
    #[must_use]
    pub fn has_cycles(&self) -> bool {
        false
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Version;

    #[test]
    fn test_add_and_find_nodes() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "zod".to_string(),
            source: SourceType::Npm,
            version: Version::parse("3.23.8").unwrap(),
            package_hash: None,
            dependencies: Vec::new(),
        });
        g.add_node(Node {
            name: "react".to_string(),
            source: SourceType::Npm,
            version: Version::parse("18.3.0").unwrap(),
            package_hash: None,
            dependencies: Vec::new(),
        });

        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.find_node("zod"), Some(0));
        assert_eq!(g.find_node("react"), Some(1));
        assert_eq!(g.find_node("missing"), None);
    }

    #[test]
    fn test_has_cycles_returns_false() {
        let g = Graph::new();
        assert!(!g.has_cycles());
    }

    #[test]
    fn test_compute_hash_returns_zeros() {
        let g = Graph::new();
        let hash = g.compute_hash();
        assert_eq!(hash.len(), 32);
        assert!(hash.iter().all(|&b| b == 0));
    }
}
