use crate::types::{SourceType, Version};
use crate::util::hash;
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
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    #[must_use]
    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Compute a hash for the graph from serialized nodes.
    #[must_use]
    pub fn compute_hash(&self) -> [u8; 32] {
        let serialized = serde_json::to_vec(&self.nodes).unwrap_or_default();
        hash::compute(&serialized)
    }

    /// Check for cycles using DFS.
    #[must_use]
    pub fn has_cycles(&self) -> bool {
        fn dfs(nodes: &[Node], v: usize, visited: &mut [bool], stack: &mut [bool]) -> bool {
            if stack[v] {
                return true;
            }
            if visited[v] {
                return false;
            }
            visited[v] = true;
            stack[v] = true;

            for dep in &nodes[v].dependencies {
                if let Some(idx) = nodes.iter().position(|n| n.name == *dep) {
                    if dfs(nodes, idx, visited, stack) {
                        return true;
                    }
                }
            }

            stack[v] = false;
            false
        }

        let mut visited = vec![false; self.nodes.len()];
        let mut stack = vec![false; self.nodes.len()];

        for i in 0..self.nodes.len() {
            if !visited[i] && dfs(&self.nodes, i, &mut visited, &mut stack) {
                return true;
            }
        }
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
    fn test_has_cycles_empty_graph() {
        let g = Graph::new();
        assert!(!g.has_cycles());
    }

    #[test]
    fn test_no_cycle_with_two_nodes() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "a".to_string(),
            source: SourceType::Npm,
            version: Version::parse("1.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["b".to_string()],
        });
        g.add_node(Node {
            name: "b".to_string(),
            source: SourceType::Npm,
            version: Version::parse("2.0.0").unwrap(),
            package_hash: None,
            dependencies: vec![],
        });
        assert!(!g.has_cycles());
    }

    #[test]
    fn test_cycle_with_two_nodes() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "a".to_string(),
            source: SourceType::Npm,
            version: Version::parse("1.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["b".to_string()],
        });
        g.add_node(Node {
            name: "b".to_string(),
            source: SourceType::Npm,
            version: Version::parse("2.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["a".to_string()],
        });
        assert!(g.has_cycles());
    }

    #[test]
    fn test_cycle_three_nodes() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "a".to_string(),
            source: SourceType::Npm,
            version: Version::parse("1.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["b".to_string()],
        });
        g.add_node(Node {
            name: "b".to_string(),
            source: SourceType::Npm,
            version: Version::parse("2.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["c".to_string()],
        });
        g.add_node(Node {
            name: "c".to_string(),
            source: SourceType::Npm,
            version: Version::parse("3.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["a".to_string()],
        });
        assert!(g.has_cycles());
    }

    #[test]
    fn test_self_loop() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "a".to_string(),
            source: SourceType::Npm,
            version: Version::parse("1.0.0").unwrap(),
            package_hash: None,
            dependencies: vec!["a".to_string()],
        });
        assert!(g.has_cycles());
    }

    #[test]
    fn test_compute_hash_returns_nonzero_for_nonempty() {
        let mut g = Graph::new();
        g.add_node(Node {
            name: "zod".to_string(),
            source: SourceType::Npm,
            version: Version::parse("3.23.8").unwrap(),
            package_hash: None,
            dependencies: Vec::new(),
        });
        let hash = g.compute_hash();
        assert_eq!(hash.len(), 32);
        assert!(hash.iter().any(|&b| b != 0));
    }

    fn make_chain_graph(n: usize) -> Graph {
        let mut g = Graph::new();
        for i in 0..n {
            let name = format!("pkg-{i:04}");
            let deps = if i + 1 < n {
                vec![format!("pkg-{:04}", i + 1)]
            } else {
                vec![]
            };
            g.add_node(Node {
                name,
                source: SourceType::Npm,
                version: Version::parse("1.0.0").unwrap(),
                package_hash: None,
                dependencies: deps,
            });
        }
        g
    }

    fn make_cyclic_graph(n: usize) -> Graph {
        let mut g = Graph::new();
        for i in 0..n {
            let name = format!("pkg-{i:04}");
            let deps = vec![format!("pkg-{:04}", (i + 1) % n)];
            g.add_node(Node {
                name,
                source: SourceType::Npm,
                version: Version::parse("1.0.0").unwrap(),
                package_hash: None,
                dependencies: deps,
            });
        }
        g
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_has_cycles_chain_100(b: &mut test::Bencher) {
        let g = make_chain_graph(100);
        b.iter(|| test::black_box(&g).has_cycles());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_has_cycles_cyclic_100(b: &mut test::Bencher) {
        let g = make_cyclic_graph(100);
        b.iter(|| test::black_box(&g).has_cycles());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_compute_hash_100(b: &mut test::Bencher) {
        let g = make_chain_graph(100);
        b.iter(|| test::black_box(&g).compute_hash());
    }
}
