use std::io::Write;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

use ara::analysis::analyzer::analyze_package;
use ara::analysis::scanner::scan_package;

fn create_analysis_dir(n: usize) -> TempDir {
    let dir = TempDir::new().expect("failed to create temp dir");
    for i in 0..n {
        let name = format!("src/file_{i:06}.js");
        let content = format!("const x = {i};\neval(x);\n");
        let full = dir.path().join(&name);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&full).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }
    dir
}

fn create_scan_dir(n: usize) -> TempDir {
    let dir = TempDir::new().expect("failed to create temp dir");
    for i in 0..n {
        let name = format!("src/file_{i:06}.js");
        let content = format!("const x = {i};\n");
        let full = dir.path().join(&name);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&full).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }
    for p in &["node_modules", ".git", "dist"] {
        std::fs::create_dir_all(dir.path().join(p)).unwrap();
    }
    std::fs::write(dir.path().join("node_modules/evil.js"), "bad").unwrap();
    std::fs::write(dir.path().join(".git/config.js"), "skip").unwrap();
    std::fs::write(dir.path().join("dist/bundle.js"), "nope").unwrap();
    dir
}

fn bench_analyze_small(c: &mut Criterion) {
    let dir = create_analysis_dir(10);
    c.bench_function("analyze_small_10files", |b| {
        b.iter(|| analyze_package(black_box(dir.path())).unwrap());
    });
}

fn bench_analyze_medium(c: &mut Criterion) {
    let dir = create_analysis_dir(100);
    c.bench_function("analyze_medium_100files", |b| {
        b.iter(|| analyze_package(black_box(dir.path())).unwrap());
    });
}

fn bench_analyze_large(c: &mut Criterion) {
    let dir = create_analysis_dir(1000);
    c.bench_function("analyze_large_1000files", |b| {
        b.iter(|| analyze_package(black_box(dir.path())).unwrap());
    });
}

fn bench_scan_small(c: &mut Criterion) {
    let dir = create_scan_dir(10);
    c.bench_function("scan_small_10files", |b| {
        b.iter(|| scan_package(black_box(dir.path())).unwrap());
    });
}

fn bench_scan_medium(c: &mut Criterion) {
    let dir = create_scan_dir(100);
    c.bench_function("scan_medium_100files", |b| {
        b.iter(|| scan_package(black_box(dir.path())).unwrap());
    });
}

fn bench_scan_large(c: &mut Criterion) {
    let dir = create_scan_dir(1000);
    c.bench_function("scan_large_1000files", |b| {
        b.iter(|| scan_package(black_box(dir.path())).unwrap());
    });
}

criterion_group!(
    name = security_scan;
    config = Criterion::default();
    targets =
        bench_analyze_small,
        bench_analyze_medium,
        bench_analyze_large,
        bench_scan_small,
        bench_scan_medium,
        bench_scan_large,
);
criterion_main!(security_scan);
