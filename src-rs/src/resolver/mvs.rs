use std::collections::HashSet;

use crate::resolver::graph::{Graph, Node};
use crate::types::{Constraint, SourceType, Version};

#[derive(Debug, Clone)]
pub struct ConstraintEntry {
    pub package: String,
    pub constraint: Constraint,
    pub source: SourceType,
    pub required_by: String,
}

#[derive(Debug)]
pub struct Resolver {
    constraints: Vec<ConstraintEntry>,
}

impl Resolver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }

    pub fn add_constraint(&mut self, entry: ConstraintEntry) {
        self.constraints.push(entry);
    }

    /// Resolve constraints into a dependency graph.
    ///
    /// For each package with constraints, selects the best version using
    /// the MVS (Minimum Version Selection) heuristic and builds a graph.
    pub fn resolve(&self) -> Graph {
        let mut graph = Graph::new();

        let mut seen: HashSet<&str> = HashSet::new();

        for c in &self.constraints {
            if !seen.insert(&c.package) {
                continue;
            }

            if let Some(version) = select_version(&self.constraints, &c.package) {
                let node = Node {
                    name: c.package.clone(),
                    source: c.source,
                    version,
                    package_hash: None,
                    dependencies: Vec::new(),
                };
                graph.add_node(node);
            }
        }

        graph
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

fn select_version(constraints: &[ConstraintEntry], package: &str) -> Option<Version> {
    let mut best: Option<Version> = None;

    for c in constraints {
        if c.package != package {
            continue;
        }

        let candidate = match &c.constraint {
            Constraint::Exact(v) => v.clone(),
            Constraint::GreaterOrEqual(v) => v.clone(),
            Constraint::GreaterThan(v) => v.clone(),
            Constraint::Caret(v) => v.clone(),
            Constraint::Tilde(v) => v.clone(),
            Constraint::LessOrEqual(_) | Constraint::LessThan(_) => continue,
            Constraint::Wildcard(_) => Version::parse("0.0.0").ok()?,
        };

        if best.as_ref().map_or(true, |b| candidate > *b) {
            best = Some(candidate);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_single_dependency() {
        let mut r = Resolver::new();
        r.add_constraint(ConstraintEntry {
            package: "zod".to_string(),
            constraint: Constraint::parse(">=3.0.0").unwrap(),
            source: SourceType::Npm,
            required_by: "root".to_string(),
        });

        let graph = r.resolve();
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn test_simple_mvs_selection() {
        let mut r = Resolver::new();
        r.add_constraint(ConstraintEntry {
            package: "c".to_string(),
            constraint: Constraint::parse(">=2.0.0").unwrap(),
            source: SourceType::Npm,
            required_by: "a".to_string(),
        });
        r.add_constraint(ConstraintEntry {
            package: "c".to_string(),
            constraint: Constraint::parse(">=2.1.0").unwrap(),
            source: SourceType::Npm,
            required_by: "b".to_string(),
        });

        let graph = r.resolve();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].name, "c");
    }
}
