use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};

use ara_util::hash;

fn bench_hash_compute_1kb(c: &mut Criterion) {
    let data = vec![0xABu8; 1024];
    c.bench_function("hash_compute_1kb", |b| {
        b.iter(|| hash::compute(black_box(&data)));
    });
}

fn bench_hash_compute_100kb(c: &mut Criterion) {
    let data = vec![0xABu8; 1024 * 100];
    c.bench_function("hash_compute_100kb", |b| {
        b.iter(|| hash::compute(black_box(&data)));
    });
}

fn bench_hash_compute_1mb(c: &mut Criterion) {
    let data = vec![0xABu8; 1024 * 1024];
    c.bench_function("hash_compute_1mb", |b| {
        b.iter(|| hash::compute(black_box(&data)));
    });
}

fn bench_hex_encode(c: &mut Criterion) {
    let h = hash::compute(b"benchmark-data-for-hex-encode");
    c.bench_function("hex_encode", |b| {
        b.iter(|| hash::hex_encode(black_box(&h)));
    });
}

criterion_group!(
    name = util;
    config = Criterion::default();
    targets =
        bench_hash_compute_1kb,
        bench_hash_compute_100kb,
        bench_hash_compute_1mb,
        bench_hex_encode,
);
criterion_main!(util);
