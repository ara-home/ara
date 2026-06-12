use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};

use ara_source::url::parse_install_spec;

fn bench_parse_npm_bare(c: &mut Criterion) {
    c.bench_function("parse_npm_bare", |b| {
        b.iter(|| parse_install_spec(black_box("zod")));
    });
}

fn bench_parse_npm_with_version(c: &mut Criterion) {
    c.bench_function("parse_npm_with_version", |b| {
        b.iter(|| parse_install_spec(black_box("zod@3.23.8")));
    });
}

fn bench_parse_npm_with_range(c: &mut Criterion) {
    c.bench_function("parse_npm_with_range", |b| {
        b.iter(|| parse_install_spec(black_box("zod@^3.0.0")));
    });
}

fn bench_parse_npm_scoped(c: &mut Criterion) {
    c.bench_function("parse_npm_scoped", |b| {
        b.iter(|| parse_install_spec(black_box("@types/node@25.9.2")));
    });
}

fn bench_parse_npm_scoped_range(c: &mut Criterion) {
    c.bench_function("parse_npm_scoped_range", |b| {
        b.iter(|| parse_install_spec(black_box("@vitejs/plugin-vue@^6.0.0")));
    });
}

fn bench_parse_github_shorthand(c: &mut Criterion) {
    c.bench_function("parse_github_shorthand", |b| {
        b.iter(|| parse_install_spec(black_box("github:user/repo")));
    });
}

fn bench_parse_github_shorthand_with_tag(c: &mut Criterion) {
    c.bench_function("parse_github_shorthand_with_tag", |b| {
        b.iter(|| parse_install_spec(black_box("github:user/repo#v1.2.3")));
    });
}

fn bench_parse_github_shorthand_with_commit(c: &mut Criterion) {
    c.bench_function("parse_github_shorthand_with_commit", |b| {
        b.iter(|| parse_install_spec(black_box("github:user/repo#abc123def456")));
    });
}

fn bench_parse_git_url(c: &mut Criterion) {
    c.bench_function("parse_git_url", |b| {
        b.iter(|| parse_install_spec(black_box("git+https://github.com/user/repo.git")));
    });
}

fn bench_parse_git_ssh(c: &mut Criterion) {
    c.bench_function("parse_git_ssh", |b| {
        b.iter(|| parse_install_spec(black_box("git+ssh://git@github.com/user/repo.git")));
    });
}

fn bench_parse_tarball_url(c: &mut Criterion) {
    c.bench_function("parse_tarball_url", |b| {
        b.iter(|| parse_install_spec(black_box("https://example.com/pkg-1.0.0.tgz")));
    });
}

fn bench_parse_tarball_tar_gz(c: &mut Criterion) {
    c.bench_function("parse_tarball_tar_gz", |b| {
        b.iter(|| parse_install_spec(black_box("https://example.com/pkg-1.0.0.tar.gz")));
    });
}

criterion_group!(
    name = source_url_parsing;
    config = Criterion::default();
    targets =
        bench_parse_npm_bare,
        bench_parse_npm_with_version,
        bench_parse_npm_with_range,
        bench_parse_npm_scoped,
        bench_parse_npm_scoped_range,
        bench_parse_github_shorthand,
        bench_parse_github_shorthand_with_tag,
        bench_parse_github_shorthand_with_commit,
        bench_parse_git_url,
        bench_parse_git_ssh,
        bench_parse_tarball_url,
        bench_parse_tarball_tar_gz,
);
criterion_main!(source_url_parsing);
