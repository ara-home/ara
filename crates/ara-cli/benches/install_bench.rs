use std::io::Write;

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

use ara_cli::cli::install::{extract_tarball, hardlink_dir, install_bin_links};
use ara_store::index::StoreIndex;

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

fn make_store() -> (TempDir, ara_store::cas::Store) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let store = ara_store::cas::Store::new(dir.path().join("store"));
    store.ensure_dirs().unwrap();
    (dir, store)
}

fn bench_extract_tarball_100(c: &mut Criterion) {
    let tarball = make_tarball(100);
    c.bench_function("extract_tarball_100", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
        });
    });
}

fn bench_extract_tarball_1000(c: &mut Criterion) {
    let tarball = make_tarball(1000);
    c.bench_function("extract_tarball_1000", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
        });
    });
}

fn bench_extract_tarball_5000(c: &mut Criterion) {
    let tarball = make_tarball(5000);
    c.bench_function("extract_tarball_5000", |b| {
        b.iter(|| {
            let tmp = TempDir::new().unwrap();
            extract_tarball(black_box(&tarball), black_box(tmp.path())).unwrap();
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
    let nodes: Vec<ara_types::Version> = (0..100)
        .map(|i| ara_types::Version::parse(&format!("{i}.0.0")).unwrap())
        .collect();
    let bytes = serde_json::to_vec(&nodes).unwrap();
    c.bench_function("store_put_graph_100", |b| {
        b.iter(|| store.put_graph(black_box(&bytes)).unwrap());
    });
}

fn create_hardlink_source(n: usize) -> TempDir {
    let dir = TempDir::new().unwrap();
    for i in 0..n {
        let sub = format!("sub{:02}", i % 10);
        let path = dir.path().join(&sub).join(format!("file_{i:06}.js"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"module.exports = {};\n").unwrap();
    }
    dir
}

fn bench_hardlink_dir_100(c: &mut Criterion) {
    let src_dir = create_hardlink_source(100);
    c.bench_function("hardlink_dir_100", |b| {
        b.iter(|| {
            let dst = TempDir::new().unwrap();
            hardlink_dir(black_box(src_dir.path()), black_box(dst.path())).unwrap();
        });
    });
}

fn bench_hardlink_dir_1000(c: &mut Criterion) {
    let src_dir = create_hardlink_source(1000);
    c.bench_function("hardlink_dir_1000", |b| {
        b.iter(|| {
            let dst = TempDir::new().unwrap();
            hardlink_dir(black_box(src_dir.path()), black_box(dst.path())).unwrap();
        });
    });
}

fn create_bin_package(n: usize) -> (TempDir, String, TempDir) {
    let pkg_dir = TempDir::new().unwrap();
    let mut bins = serde_json::Map::new();
    for i in 0..n {
        let bin_name = format!("tool-{i:04}");
        let bin_path = format!("bin/{i:04}.js");
        bins.insert(bin_name, serde_json::Value::String(bin_path.clone()));
        let full = pkg_dir.path().join(&bin_path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&full).unwrap();
        f.write_all(b"#!/usr/bin/env node\nconsole.log('ok');\n")
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let json = serde_json::json!({"name": "bench-pkg", "bin": bins});
    std::fs::write(pkg_dir.path().join("package.json"), json.to_string()).unwrap();

    let nm_dir = TempDir::new().unwrap();
    std::fs::create_dir_all(nm_dir.path().join(".bin")).unwrap();

    (pkg_dir, "bench-pkg".to_string(), nm_dir)
}

fn bench_install_bin_links_10(c: &mut Criterion) {
    let (pkg_dir, pkg_name, nm_dir) = create_bin_package(10);
    c.bench_function("install_bin_links_10", |b| {
        b.iter(|| {
            install_bin_links(
                black_box(nm_dir.path()),
                black_box(&pkg_name),
                black_box(pkg_dir.path()),
            )
            .unwrap();
        });
    });
}

fn bench_install_bin_links_50(c: &mut Criterion) {
    let (pkg_dir, pkg_name, nm_dir) = create_bin_package(50);
    c.bench_function("install_bin_links_50", |b| {
        b.iter(|| {
            install_bin_links(
                black_box(nm_dir.path()),
                black_box(&pkg_name),
                black_box(pkg_dir.path()),
            )
            .unwrap();
        });
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

criterion_group!(
    name = install_phase3b;
    config = Criterion::default();
    targets =
        bench_hardlink_dir_100,
        bench_hardlink_dir_1000,
        bench_install_bin_links_10,
        bench_install_bin_links_50,
);

fn make_index() -> (TempDir, StoreIndex) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let index = StoreIndex::new(dir.path().join("index.db")).unwrap();
    (dir, index)
}

fn make_index_entries(n: usize) -> Vec<(String, String, String, i64)> {
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        entries.push((
            format!("npm:pkg-{i:04}@1.0.0"),
            format!("sha256-{:064x}", i),
            "npm".to_string(),
            1024 + i as i64 * 10,
        ));
    }
    entries
}

fn bench_index_insert_individual_50(c: &mut Criterion) {
    let entries = make_index_entries(50);
    c.bench_function("index_insert_individual_50", |b| {
        b.iter(|| {
            let (_dir, index) = make_index();
            for (ck, hash, source, size) in &entries {
                index.insert(ck, hash, source, *size).unwrap();
            }
        });
    });
}

fn bench_index_batch_insert_50(c: &mut Criterion) {
    let entries = make_index_entries(50);
    c.bench_function("index_batch_insert_50", |b| {
        b.iter(|| {
            let (_dir, index) = make_index();
            index.batch_insert(black_box(&entries)).unwrap();
        });
    });
}

fn bench_index_batch_insert_200(c: &mut Criterion) {
    let entries = make_index_entries(200);
    c.bench_function("index_batch_insert_200", |b| {
        b.iter(|| {
            let (_dir, index) = make_index();
            index.batch_insert(black_box(&entries)).unwrap();
        });
    });
}

fn bench_index_lookup_50(c: &mut Criterion) {
    let (_dir, index) = make_index();
    let entries = make_index_entries(50);
    index.batch_insert(&entries).unwrap();
    c.bench_function("index_lookup_50", |b| {
        b.iter(|| {
            for (ck, ..) in &entries {
                let _ = index.lookup(ck);
            }
        });
    });
}

criterion_group!(
    name = install_store_index;
    config = Criterion::default();
    targets =
        bench_index_insert_individual_50,
        bench_index_batch_insert_50,
        bench_index_batch_insert_200,
        bench_index_lookup_50,
);

criterion_main!(install_phase1, install_phase3b, install_store_index);
