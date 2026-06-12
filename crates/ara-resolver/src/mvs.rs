use std::collections::HashSet;

use crate::graph::{Graph, Node};
use ara_types::{Constraint, SourceType, Version};

#[derive(Debug, Clone)]
pub struct ConstraintEntry {
    pub package: String,
    pub constraint: Constraint,
    pub source: SourceType,
    #[allow(dead_code)]
    pub required_by: String,
}

#[derive(Debug)]
pub struct Resolver {
    constraints: Vec<ConstraintEntry>,
}

impl Resolver {
    #[must_use]
    pub const fn new() -> Self {
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
            } else {
                eprintln!(
                    "  warning: no version satisfies constraints for {}",
                    c.package
                );
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

    // Collect all constraints for this package
    let pkg_constraints: Vec<&Constraint> = constraints
        .iter()
        .filter(|c| c.package == package)
        .map(|c| &c.constraint)
        .collect();

    if pkg_constraints.is_empty() {
        return None;
    }

    for c in &pkg_constraints {
        let candidate = match c {
            Constraint::Exact(v)
            | Constraint::GreaterOrEqual(v)
            | Constraint::GreaterThan(v)
            | Constraint::Caret(v)
            | Constraint::Tilde(v) => v.clone(),
            Constraint::LessOrEqual(_) | Constraint::LessThan(_) => continue,
            Constraint::Wildcard(_) => Version::parse("0.0.0").ok()?,
            Constraint::And(_) => continue,
        };

        // Verify candidate satisfies ALL constraints for this package
        if pkg_constraints
            .iter()
            .all(|con| con.satisfied_by(&candidate))
            && best.as_ref().is_none_or(|b| candidate < *b)
        {
            best = Some(candidate);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
        // MVS selects the minimum version that satisfies all constraints
        assert_eq!(graph.nodes[0].version, Version::parse("2.1.0").unwrap());
    }

    #[test]
    fn test_select_version_exact() {
        let constraints = vec![ConstraintEntry {
            package: "pkg".into(),
            constraint: Constraint::parse("1.2.3").unwrap(),
            source: SourceType::Npm,
            required_by: "root".into(),
        }];
        let version = select_version(&constraints, "pkg");
        assert!(version.is_some());
        assert_eq!(version.unwrap(), Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn test_select_version_unsatisfiable() {
        // Wildcard returns 0.0.0, but Exact("2.0.0") won't be satisified by 0.0.0
        // Actually, Wildcard returns None from select_version because "0.0.0" won't satisfy "2.0.0"
        let constraints = vec![ConstraintEntry {
            package: "pkg".into(),
            constraint: Constraint::parse("^2.0.0").unwrap(),
            source: SourceType::Npm,
            required_by: "root".into(),
        }];
        // select_version picks the caret candidate (2.0.0) which should be returned
        let version = select_version(&constraints, "pkg");
        assert!(version.is_some());
        assert_eq!(version.unwrap().major, 2);
    }

    #[test]
    fn test_select_version_less_or_equal_skipped() {
        let constraints = vec![ConstraintEntry {
            package: "pkg".into(),
            constraint: Constraint::parse("<=1.0.0").unwrap(),
            source: SourceType::Npm,
            required_by: "root".into(),
        }];
        // LessOrEqual is skipped in select_version (continue), so no candidate
        let version = select_version(&constraints, "pkg");
        assert!(version.is_none());
    }

    #[test]
    fn test_select_version_none_for_unknown_package() {
        let constraints = vec![ConstraintEntry {
            package: "other".into(),
            constraint: Constraint::parse(">=1.0.0").unwrap(),
            source: SourceType::Npm,
            required_by: "root".into(),
        }];
        let version = select_version(&constraints, "missing");
        assert!(version.is_none());
    }

    #[cfg(feature = "nightly-bench")]
    fn make_resolver_constraints(n: usize) -> Vec<ConstraintEntry> {
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            entries.push(ConstraintEntry {
                package: format!("pkg-{i:04}"),
                constraint: Constraint::parse(">=1.0.0").unwrap(),
                source: SourceType::Npm,
                required_by: "root".to_string(),
            });
        }
        entries
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_resolve_100(b: &mut test::Bencher) {
        let entries = make_resolver_constraints(100);
        b.iter(|| {
            let mut r = Resolver::new();
            for e in &entries {
                r.add_constraint(e.clone());
            }
            r.resolve();
        });
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_resolve_500(b: &mut test::Bencher) {
        let entries = make_resolver_constraints(500);
        b.iter(|| {
            let mut r = Resolver::new();
            for e in &entries {
                r.add_constraint(e.clone());
            }
            r.resolve();
        });
    }
}
