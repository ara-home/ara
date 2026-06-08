use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

fn make_tarball(n: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
    let mut ar = tar::Builder::new(encoder);
    for i in 0..n {
        let name = format!("files/file_{i:06}.js");
        let content = format!("module.exports = {{ id: {i} }};\n");
        let mut header = tar::Header::new_gnu();
        header.set_path(&name).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, content.as_bytes()).unwrap();
    }
    let encoder = ar.into_inner().unwrap();
    encoder.finish().unwrap();
    buf
}

fn make_store() -> (TempDir, ara::store::cas::Store) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let store = ara::store::cas::Store::new(dir.path().join("store"));
    store.ensure_dirs().unwrap();
    (dir, store)
}

fn bench_extract_tarball_100(c: &mut Criterion) {
    let tarball = make_tarball(100);
    c.bench_function("extract_tarball_100", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            ara::cli::install::extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
        });
    });
}

fn bench_extract_tarball_1000(c: &mut Criterion) {
    let tarball = make_tarball(1000);
    c.bench_function("extract_tarball_1000", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            ara::cli::install::extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
        });
    });
}

fn bench_extract_tarball_5000(c: &mut Criterion) {
    let tarball = make_tarball(5000);
    c.bench_function("extract_tarball_5000", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            ara::cli::install::extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
        });
    });
}

fn bench_store_put_1kb(c: &mut Criterion) {
    let (_dir, store) = make_store();
    let data = vec![0u8; 1024];
    c.bench_function("store_put_1kb", |b| {
        b.iter(|| store.put(black_box(&data)).unwrap());
    });
}

fn bench_store_put_100kb(c: &mut Criterion) {
    let (_dir, store) = make_store();
    let data = vec![0u8; 1024 * 100];
    c.bench_function("store_put_100kb", |b| {
        b.iter(|| store.put(black_box(&data)).unwrap());
    });
}

fn bench_store_get(c: &mut Criterion) {
    let (_dir, store) = make_store();
    let hash = store.put(b"benchmark-data").unwrap();
    c.bench_function("store_get", |b| {
        b.iter(|| store.get(black_box(&hash)).unwrap());
    });
}

fn bench_store_put_graph_100(c: &mut Criterion) {
    let (_dir, store) = make_store();
    let nodes: Vec<ara::types::Version> = (0..100)
        .map(|i| ara::types::Version::parse(&format!("{i}.0.0")).unwrap())
        .collect();
    let bytes = serde_json::to_vec(&nodes).unwrap();
    c.bench_function("store_put_graph_100", |b| {
        b.iter(|| store.put_graph(black_box(&bytes)).unwrap());
    });
}

criterion_group!(
    name = install_phase1;
    config = Criterion::default();
    targets =
        bench_extract_tarball_100,
        bench_extract_tarball_1000,
        bench_extract_tarball_5000,
        bench_store_put_1kb,
        bench_store_put_100kb,
        bench_store_get,
        bench_store_put_graph_100,
);
criterion_main!(install_phase1);
