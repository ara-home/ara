# Changelog

All notable changes to this project will be documented in this file.

## [0.9.1] - 2026-06-08

### 🚀 Features

- *(install)* Add --package-lock flag to generate package-lock.json (#27)

### 📚 Documentation

- Document --package-lock flag in README

## [0.9.0] - 2026-06-08

### 🚀 Features

- Reduce security scanner noise by skipping .d.ts files and auto-approving low-risk packages (#25)

### ⚡ Performance

- Speed up install phase 3b by 7x with HTTP/2 warmup, larger window, and batch SQLite inserts (#26)

## [0.8.0] - 2026-06-08

### 🚀 Features

- Add CodSpeed continuous benchmarking CI and add +3 fixtures e2e tests (#24)

### ⚡ Performance

- Optimize Phase 3b installation with concurrency and streaming extraction (#23)

## [0.7.0] - 2026-06-07

### 🐛 Bug Fixes

- Prevent silent data loss in legacy index migration and lockfile reads (#17)
- Propagate write_lockfile errors and gate graph cleanup behind --aggressive (#18)
- Add warnings to silent error paths in install and resolver (#19)
- Distinguish 404, parse errors, and network errors in registry source (#20)
- Add input validation for manifest and lockfile (#21)

### 🧪 Testing

- Add more tests (#22)

## [0.6.0] - 2026-06-07

### 🚀 Features

- Atomic operations, integrity checks, sharding, SQLite index, and full GC (#15)
- Parse workspace: prefix, live symlinks, and e2e tests (#16)

### 🐛 Bug Fixes

- Translate Portuguese examples to English

### 📚 Documentation

- Document workspace protocol, live symlinks, and hybrid manifests

## [0.5.0] - 2026-06-06

### 🐛 Bug Fixes

- Correct tarball URL for scoped npm packages (@scope/name) (#12)
- Build on Windows

## [0.4.1] - 2026-06-05

### 🐛 Bug Fixes

- Respect dist-tags.latest and fix tarball URLs with prerelease (#11)

### 📚 Documentation

- Document direct package install (ara install <spec>)

### ⚙️ Miscellaneous Tasks

- Add install target for local binary

## [0.4.0] - 2026-06-05

### 🚀 Features

- Implement ara install <spec> for direct package install (RFC-002) (#10)

## [0.3.0] - 2026-06-05

### 🚀 Features

- Add fixture-based test harness with 39 scenarios (#8)

### 📚 Documentation

- Update README

## [0.2.0] - 2026-06-04

### 🚀 Features

- Add kind field to DependencyEntry and package.json parser module
- Auto-detect package.json in install command
- Add package.json generator

### 🐛 Bug Fixes

- Escape TOML output and support workspace object form

## [0.1.0] - 2026-06-04

### 🚀 Features

- Add ara-sec Rust security engine (Phase 1 & 2)
- Analyze/audit CLI commands with ara-sec integration
- Bundle script and side-by-side binary delivery
- Add src-rs Rust foundation (types + hash) alongside Zig
- Port manifest, lockfile, store (CAS), resolver (graph + MVS)
- Port sources (local, workspace, git, github, registry) + HTTP client
- Port sandbox (profiles + executor with Linux seccomp)
- Port CLI (analyze, audit, install, run) with clap
- Wire sandbox into run, add store cache + graph_hash + security meta
- Connect source::resolve, has_cycles, compute_hash, gc command

### 🐛 Bug Fixes

- Adapt to Zig 0.13 API changes
- Correct MVS algorithm, propagate hash errors, add supply-chain CI
- Block path traversal in CAS store, add HTTP retry
- Change native-tls to rustls-tls to eliminate openssl dependency

### 📚 Documentation

- Add README

### 🧪 Testing

- Add error, allocator, generative, and comptime tests

### ⚙️ Miscellaneous Tasks

- Add Makefile, tests, fixtures, CI pipeline
- Add cargo-dist, license, format checks
