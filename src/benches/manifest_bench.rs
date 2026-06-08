use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::json;

use ara::manifest::package_json;
use ara::manifest::parser;

fn make_package_json(n: usize) -> String {
    let mut deps = serde_json::Map::new();
    for i in 0..n {
        deps.insert(format!("dep-{i:04}"), json!(format!("^{}.0.0", i % 10)));
    }
    let val = json!({
        "name": "bench-pkg",
        "version": "1.0.0",
        "dependencies": deps,
    });
    val.to_string()
}

fn make_ara_toml(n: usize) -> String {
    let mut deps = String::new();
    for i in 0..n {
        deps.push_str(&format!(
            "\"dep-{i:04}\" = {{ source = \"npm\", version = \">=1.0.0\" }}\n"
        ));
    }
    format!(
        r#"[project]
name = "bench-pkg"
version = "1.0.0"

[deps]
{deps}"#
    )
}

fn bench_parse_package_json_50(c: &mut Criterion) {
    let content = make_package_json(50);
    c.bench_function("parse_package_json_50deps", |b| {
        b.iter(|| package_json::parse_package_json(black_box(&content)));
    });
}

fn bench_parse_package_json_200(c: &mut Criterion) {
    let content = make_package_json(200);
    c.bench_function("parse_package_json_200deps", |b| {
        b.iter(|| package_json::parse_package_json(black_box(&content)));
    });
}

fn bench_parse_ara_toml_50(c: &mut Criterion) {
    let content = make_ara_toml(50);
    c.bench_function("parse_ara_toml_50deps", |b| {
        b.iter(|| parser::parse(black_box(&content)));
    });
}

fn bench_parse_ara_toml_200(c: &mut Criterion) {
    let content = make_ara_toml(200);
    c.bench_function("parse_ara_toml_200deps", |b| {
        b.iter(|| parser::parse(black_box(&content)));
    });
}

fn bench_generate_package_json_50(c: &mut Criterion) {
    let content = make_package_json(50);
    let manifest = package_json::parse_package_json(&content).unwrap();
    c.bench_function("generate_package_json_50deps", |b| {
        b.iter(|| package_json::generate_package_json(black_box(&manifest)));
    });
}

fn bench_generate_package_json_200(c: &mut Criterion) {
    let content = make_package_json(200);
    let manifest = package_json::parse_package_json(&content).unwrap();
    c.bench_function("generate_package_json_200deps", |b| {
        b.iter(|| package_json::generate_package_json(black_box(&manifest)));
    });
}

criterion_group!(
    name = manifest;
    config = Criterion::default();
    targets =
        bench_parse_package_json_50,
        bench_parse_package_json_200,
        bench_parse_ara_toml_50,
        bench_parse_ara_toml_200,
        bench_generate_package_json_50,
        bench_generate_package_json_200,
);
criterion_main!(manifest);
