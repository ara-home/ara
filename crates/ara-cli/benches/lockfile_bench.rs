use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};

use ara_lockfile::{generator, parser, types::Lockfile};

fn make_lockfile_toml(n: usize) -> String {
    let mut packages = String::new();
    for i in 0..n {
        packages.push_str(&format!(
            r#"[[package]]
name = "pkg-{i:04}"
version = "{version}.0.0"
source = "npm"
package_hash = "sha256-{hash:064x}"
dependencies = ["dep-{i:04}"]

"#,
            version = i % 20,
            hash = i * 0xDEADBEEF
        ));
    }
    format!(
        r#"version = 1

[graph]
resolver = "mvs"
generated_at = "2026-01-01T00:00:00Z"
graph_hash = "abcd1234"

{packages}"#
    )
}

fn make_lockfile(n: usize) -> Lockfile {
    let toml = make_lockfile_toml(n);
    parser::parse(&toml).unwrap()
}

fn bench_parse_lockfile_50(c: &mut Criterion) {
    let content = make_lockfile_toml(50);
    c.bench_function("parse_lockfile_50", |b| {
        b.iter(|| parser::parse(black_box(&content)));
    });
}

fn bench_parse_lockfile_200(c: &mut Criterion) {
    let content = make_lockfile_toml(200);
    c.bench_function("parse_lockfile_200", |b| {
        b.iter(|| parser::parse(black_box(&content)));
    });
}

fn bench_generate_lockfile_50(c: &mut Criterion) {
    let lockfile = make_lockfile(50);
    c.bench_function("generate_lockfile_50", |b| {
        b.iter(|| generator::generate(black_box(&lockfile)));
    });
}

fn bench_generate_lockfile_200(c: &mut Criterion) {
    let lockfile = make_lockfile(200);
    c.bench_function("generate_lockfile_200", |b| {
        b.iter(|| generator::generate(black_box(&lockfile)));
    });
}

criterion_group!(
    name = lockfile;
    config = Criterion::default();
    targets =
        bench_parse_lockfile_50,
        bench_parse_lockfile_200,
        bench_generate_lockfile_50,
        bench_generate_lockfile_200,
);
criterion_main!(lockfile);
