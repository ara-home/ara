
## [unreleased]

### 🚀 Features

- *(cli)* Add --profile flag to ara x with OS-aware default (#66)

### 🐛 Bug Fixes

- *(cli)* Resolve symlink-based TOCTOU in tarball extraction (#64)
- *(cli)* Prevent tar unpack from following symlinks (#65)

### 📚 Documentation

- Update CHANGELOG.md (#62)
## [0.13.1] - 2026-06-16

### 💼 Other

- *(sandbox)* [Fix security issues in sandbox] seccomp-BPF, path traversal, and more (#61)
- Bump to v0.13.1

### 📚 Documentation

- Update CHANGELOG.md (#45)
## [0.13.0] - 2026-06-15

### 🚀 Features

- Add workspace catalog support and related CLI commands (#59)

### 🐛 Bug Fixes

- *(cli)* Isolate store per test run and scope cache key with registry URL (#47)

### 💼 Other

- Add instructions for AI agents (#46)
- Bump to v0.13.0

### 🚜 Refactor

- Split install.rs (#60)

### 📚 Documentation

- Update CHANGELOG.md (#41)
## [0.12.0] - 2026-06-14

### 🚀 Features

- Improved parser error messages and security performance (#42)
- Add real-time progress bars during install (#43)

### 🐛 Bug Fixes

- Handle shorthand versions and operator whitespace in constraint parsing (#38)
- *(security)* Sandbox hardening, HTTP enforcement, tarball/file/workspace validation, SRI propagation (#44)

### 🚜 Refactor

- Migrate to Cargo Workspaces with multi-crate structure (#40)

### 📚 Documentation

- Update CHANGELOG.md (#36)
## [0.11.0] - 2026-06-09

### 🚀 Features

- Add changelog bot workflow (#35)

### 💼 Other

- Bump to v0.11.0

### ⚙️ Miscellaneous Tasks

- Update changelog
## [0.10.1] - 2026-06-09

### 🐛 Bug Fixes

- Respect version constraints and handle prerelease/compound ranges (#34)
## [0.10.0] - 2026-06-09

### 🐛 Bug Fixes

- *(deps)* Update rust crate toml to v1 (#32)

### 💼 Other

- Bump to v0.10.0

### 📚 Documentation

- Populate CHANGELOG with all releases from v0.1.0 to v0.9.1

### ⚙️ Miscellaneous Tasks

- Add Renovate dependency update config
## [0.9.1] - 2026-06-08

### 🚀 Features

- *(install)* Add --package-lock flag to generate package-lock.json (#27)

### 💼 Other

- Bump to v0.9.1

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

### 💼 Other

- Bump to v0.8.0

### ⚡ Performance

- Optimize Phase 3b installation with concurrency and streaming extraction (#23)
## [0.7.0] - 2026-06-07

### 🐛 Bug Fixes

- *(robustness)* Prevent silent data loss in legacy index migration and lockfile reads (#17)
- *(robustness)* Propagate write_lockfile errors and gate graph cleanup behind --aggressive (#18)
- *(robustness)* Add warnings to silent error paths in install and resolver (#19)
- Distinguish 404, parse errors, and network errors in registry source (#20)
- Add input validation for manifest and lockfile (#21)

### 💼 Other

- Bump to v0.7.0

### 🧪 Testing

- Add more tests (#22)
## [0.6.0] - 2026-06-07

### 🚀 Features

- *(store-efficiency)* Atomic operations, integrity checks, sharding, SQLite index, and full GC (#15)
- *(workspace-protocol)* Parse workspace: prefix, live symlinks, and e2e tests (#16)

### 🐛 Bug Fixes

- *(readme)* Translate Portuguese examples to English

### 💼 Other

- Bump to v0.6.0

### 📚 Documentation

- *(readme)* Document workspace protocol, live symlinks, and hybrid manifests
## [0.5.0] - 2026-06-06

### 🐛 Bug Fixes

- *(registry)* Correct tarball URL for scoped npm packages (@scope/name) (#12)
- Build windows
## [0.4.1] - 2026-06-05

### 🐛 Bug Fixes

- *(registry)* Respect dist-tags.latest and fix tarball URLs with prerelease (#11)

### 💼 Other

- Bump to v0.4.1"

### 📚 Documentation

- *(readme)* Document direct package install (ara install <spec>)

### ⚙️ Miscellaneous Tasks

- *(makefile)* Add install target for local binary
## [0.4.0] - 2026-06-05

### 🚀 Features

- Implement ara install <spec> for direct package install (RFC-002) (#10)

### 💼 Other

- Bump to v0.4.0
## [0.3.0] - 2026-06-05

### 🚀 Features

- *(tests)* Add fixture-based test harness with 39 scenarios (#8)

### 💼 Other

- Bump to v0.3.0

### 📚 Documentation

- Uodate readme
## [0.2.0] - 2026-06-04

### 🚀 Features

- *(manifest)* Add kind field to DependencyEntry
- *(manifest)* Add package.json parser module
- *(cli)* Auto-detect package.json in install command
- *(manifest)* Add package.json generator and remove dead code warnings

### 🐛 Bug Fixes

- *(manifest)* Escape TOML output and support workspace object form

### 💼 Other

- Bump to v0.2.0

### ⚙️ Miscellaneous Tasks

- Fix multilines
## [0.1.0] - 2026-06-04

### 🚀 Features

- Add ara-sec Rust security engine (Phase 1)
- *(ara-sec)* Implement static analysis engine (Phase 2)
- *(cli)* Add analyze/audit commands with ara-sec integration
- Bundle script and side-by-side binary delivery
- Add src-rs Rust foundation (types + hash) alongside Zig
- Move ara-sec analysis engine into src-rs
- Port manifest and lockfile (types + parser + generator)
- Port store (CAS) and resolver (graph + MVS)
- Port sources (local, workspace, git, github, registry) + HTTP client
- Port sandbox (profiles + executor with Linux seccomp)
- Port CLI (analyze, audit, install, run) with clap, replace IPC with direct analysis calls
- Wire sandbox into run, add store cache + graph_hash + security meta to install, parser validation, clean dead code
- Connect source::resolve, has_cycles, compute_hash, gc command, find_node, and parser validation
- Add cliff

### 🐛 Bug Fixes

- Adapt to Zig 0.13 API changes (writeFile, epoch, getenv, Child.init)
- E2e and unit tests passing
- Pre-commit hook path (remove bogus /ara suffix)
- Use 'zig test src/main.zig' instead of 'zig build test' for speed
- *(resolver)* Correct MVS algorithm, propagate hash errors, add supply-chain CI
- *(security)* Block path traversal in CAS store, add HTTP retry, unify analyze/audit and add build inject version
- Pre-commit
- *(ci)* Remove working directory
- Change native-tls to rustls-tls, to eliminate deps of open ssl
- Sec
- Deny

### 💼 Other

- Initial project structure with build.zig and skeleton
- Add fundamental types, version, constraint, source types, and hash utility
- Add minimal TOML parser with table and array-of-tables support
- Add inline table parsing with memory management
- Add array value support in parser
- Add manifest parser with project, deps, workspace, scripts, security, build
- Add parser and generator for ara.lock
- Implement content-addressed storage with put/get/dedup
- Add source abstraction with workspace, local, git, github, registry stubs
- Implement MVS resolver with constraint collection and graph building
- Add CLI entrypoint with install, run commands and argument parsing
- Add sandbox profiles (open/restricted/hermetic/custom) and executor
- Implement HTTP client with std.http.Client
- Implement real fetch for all sources (HTTP, git CLI, local)
- Complete pipeline with fetch, CAS store, materialize, and lockfile
- Add JSON-RPC layer for Zig ↔ Rust subprocess communication
- Add workspace to lsp resolver
- Testes, clippy, lint, doc comments e Makefile para src-rs

### 🚜 Refactor

- Replace hand-rolled date math, semver parser, and unify source types

### 📚 Documentation

- Add readme

### 🧪 Testing

- Add error, allocator, generative, and comptime tests

### ⚙️ Miscellaneous Tasks

- Add make file, tests and fixtures
- Update test
- Remove ara-sec from pre commit hook
- Format code
- Format code
- Remove legacy code and prepare codebase
- Resolve dead code
- Format code
- Organize src
- Add fmt check in pre commit hook
- Add step check fmt in hook
- Add license
- Add cargo-dist
- Add ci pipeline"
- Change repo owner
- Run release automatically
