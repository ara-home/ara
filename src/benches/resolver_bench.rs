use criterion::{black_box, criterion_group, criterion_main, Criterion};

use ara::resolver::graph::Graph;
use ara::resolver::mvs::{ConstraintEntry, Resolver};
use ara::types::Version;
use ara::types::{Constraint, SourceType};

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

fn make_chain_graph(n: usize) -> Graph {
    let mut g = Graph::new();
    for i in 0..n {
        let name = format!("pkg-{i:04}");
        let deps = if i + 1 < n {
            vec![format!("pkg-{:04}", i + 1)]
        } else {
            vec![]
        };
        g.add_node(ara::resolver::graph::Node {
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
        g.add_node(ara::resolver::graph::Node {
            name,
            source: SourceType::Npm,
            version: Version::parse("1.0.0").unwrap(),
            package_hash: None,
            dependencies: deps,
        });
    }
    g
}

fn bench_resolve_100(c: &mut Criterion) {
    let entries = make_resolver_constraints(100);
    c.bench_function("resolve_100", |b| {
        b.iter(|| {
            let mut r = Resolver::new();
            for e in black_box(&entries) {
                r.add_constraint(e.clone());
            }
            r.resolve();
        });
    });
}

fn bench_resolve_500(c: &mut Criterion) {
    let entries = make_resolver_constraints(500);
    c.bench_function("resolve_500", |b| {
        b.iter(|| {
            let mut r = Resolver::new();
            for e in black_box(&entries) {
                r.add_constraint(e.clone());
            }
            r.resolve();
        });
    });
}

fn bench_resolve_2000(c: &mut Criterion) {
    let entries = make_resolver_constraints(2000);
    c.bench_function("resolve_2000", |b| {
        b.iter(|| {
            let mut r = Resolver::new();
            for e in black_box(&entries) {
                r.add_constraint(e.clone());
            }
            r.resolve();
        });
    });
}

fn bench_graph_has_cycles_chain_100(c: &mut Criterion) {
    let g = make_chain_graph(100);
    c.bench_function("graph_has_cycles_chain_100", |b| {
        b.iter(|| black_box(&g).has_cycles());
    });
}

fn bench_graph_has_cycles_cyclic_100(c: &mut Criterion) {
    let g = make_cyclic_graph(100);
    c.bench_function("graph_has_cycles_cyclic_100", |b| {
        b.iter(|| black_box(&g).has_cycles());
    });
}

fn bench_graph_compute_hash_100(c: &mut Criterion) {
    let g = make_chain_graph(100);
    c.bench_function("graph_compute_hash_100", |b| {
        b.iter(|| black_box(&g).compute_hash().ok());
    });
}

criterion_group!(
    name = resolver_phase3a;
    config = Criterion::default();
    targets =
        bench_resolve_100,
        bench_resolve_500,
        bench_resolve_2000,
        bench_graph_has_cycles_chain_100,
        bench_graph_has_cycles_cyclic_100,
        bench_graph_compute_hash_100,
);
criterion_main!(resolver_phase3a);
