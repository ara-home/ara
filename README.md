# Ara

A dependency manager that actually thinks about security, so you don't have to.

Ara is a modern package manager and build orchestrator built for the JavaScript and TypeScript ecosystem — but with a hard focus on determinism, security, and reproducibility. It takes lessons from Go Modules, pnpm, Nix, and Cargo, wraps them in a familiar CLI, and adds a built-in security analysis engine that inspects every dependency before it touches your project.

Think of it as npm reimagined from scratch for an era where supply chain attacks are the norm, not the exception.

---

## What makes Ara different?

### Security analysis baked in, not bolted on

Every time you install a dependency, Ara scans its source files for suspicious patterns before unpacking them. It looks for eval() calls, child_process invocations, prototype pollution, credential access, obfuscated code, and a dozen other red flags — then shows you exactly what it found and asks for your decision before proceeding.

```bash
ara install
```

If a package tries to run `eval()` or access `process.binding()`, Ara tells you. Right there, in your terminal. Not in a CI pipeline you set up six months later.

### Interactive prompts, non-interactive when you need it

By default, Ara asks you to approve or deny each package with findings. For CI environments, just pass `--non-interactive` and it installs silently.

```bash
ara install --non-interactive
```

### Content-addressed storage

Every package is stored by its SHA-256 hash, not by name and version. This means:
- Identical packages are never duplicated
- You can verify package integrity without external tooling
- Rollbacks are trivial — the old hash still exists in the store

### Deterministic resolution with MVS

Ara uses Minimum Version Selection, inspired by Go Modules. Given the same manifest and lockfile, any machine produces the exact same dependency graph. No floating versions, no surprises.

### Sandboxed script execution

When you run a build or test script, Ara wraps it in a Linux seccomp-BPF filter that restricts what syscalls the process can make. Three profiles are available:

- **Hermetic** — minimal syscall set, no network, deterministic clock (great for builds)
- **Restricted** — safe syscalls, read-only filesystem, no network
- **Open** — no restrictions (for trusted scripts)

```bash
ara run build --profile hermetic
```

### Multiple package sources

Ara can resolve and fetch dependencies from npm registries, GitHub repositories, git repositories, tarball URLs, local paths, and workspace members — all defined in a single manifest file or installed directly via spec.

### Workspace protocol

Ara supports the `workspace:` protocol (inspired by pnpm), which lets you declare dependencies on sibling workspace members without reaching out to a registry:

```json
{
  "name": "monorepo",
  "private": true,
  "workspaces": ["packages/*"],
  "dependencies": {
    "lib-a": "workspace:^",
    "lib-b": "workspace:*",
    "zod": "^3.0.0"
  }
}
```

Supported forms:

| Form | Meaning |
|------|---------|
| `workspace:*` | Always resolves to the local workspace member |
| `workspace:^` | Resolves to the member and will be replaced with `^<version>` on publish |
| `workspace:1.2.3` | Pins to exact version in the workspace member |

When installing, workspace dependencies become **live symlinks** — `node_modules/lib-a` points directly to `packages/lib-a`. Changes to the member source files are immediately visible to consumers, no reinstall needed.

The root `package.json` can mix workspace and npm deps freely. The lockfile records each dep with its correct `source`:
- Workspace deps → `source = "workspace"`
- Registry deps → `source = "registry"`

---

## Architecture

Ara is written in Rust and organized as a single binary with these core subsystems:

```
src/
├── main.rs              # Entry point, CLI dispatch
├── cli/                 # Install, run, analyze, audit, gc
├── types.rs             # Version, Constraint, RiskLevel, SourceType
├── manifest/            # Manifest parsing: ara.toml + package.json
├── lockfile/            # Lockfile types and generation
├── resolver/            # MVS resolver + dependency graph with cycle detection
├── source/              # Package source backends (npm, git, github, local, workspace)
├── store/               # Content-addressable store (put/get by SHA-256)
├── analysis/            # Security scanner + pattern-based analyzer
├── sandbox/             # Seccomp-BPF sandbox execution profiles
└── util/                # Hashing, HTTP client helpers
```

### The install flow

#### Manifest install (`ara install` with no args)

1. **Parse** the manifest — reads `package.json` as the primary source for dependencies and scripts, and merges advanced settings from `ara.toml` if it exists.
2. **Expand workspace members** — globs `workspaces` patterns and creates implicit deps for each discovered member
3. **Resolve** each dependency using MVS — select the best version that satisfies all constraints
4. **Fetch or symlink**: workspace deps become **live symlinks** from `node_modules/<name>` to the member directory; all other sources fetch tarballs from the appropriate backend
5. **Analyze** every package by scanning its source files against 16+ security patterns (including symlinked workspace members)
6. **Prompt** the user if suspicious code is found (unless `--non-interactive`)
7. **Extract** approved packages to the output directory and store them in the content-addressable store
8. **Lock** the resolved graph into `ara.lock` for future reproducibility

#### Direct spec install (`ara install <spec>`)

1. **Parse** the spec string — determine target type (npm, GitHub, git, tarball)
2. **Resolve** the version — query registry for latest/concrete, or use provided ref directly
3. **Check cache** — skip download if already cached (unless `--force`/`--refresh`)
4. **Fetch** the tarball from the appropriate backend
5. **Analyze** the package by scanning source files for suspicious patterns
6. **Prompt** the user if suspicious code is found (unless `--non-interactive`)
7. **Extract** to `node_modules/` and store in the content-addressable store
8. **Update** `package.json` with the newly installed packages.
9. **Write** the `ara.lock` file.

### The analysis engine

The scanner walks every JavaScript, TypeScript, JSX, TSX, MJS, CJS, MTS, and CTS file in a package, skipping binary files, large files (>500 KB), and known ignore directories (`node_modules/`, `.git/`, `dist/`, etc.).

The analyzer then runs each file through a set of compiled regex patterns. Currently it detects:

| Pattern | Severity | What it catches |
|---|---|---|
| `eval-usage` | Critical | Arbitrary code execution via `eval()` |
| `new-function` | Critical | Dynamic code creation via `new Function()` |
| `child-process-exec` | High | Shell command execution, potential injection |
| `child-process-require` | High | Import of `child_process` module |
| `vm-escape` | High | VM sandbox escape methods |
| `process-binding` | High | Access to native addons |
| `prototype-pollution` | High | `__proto__` assignment |
| `constructor-pollution` | High | `constructor.prototype` manipulation |
| `credential-access` | High | Access to `process.env`, `AWS_*`, tokens |
| `obfuscated-code` | Medium | Base64, hex-encoded, or compressed strings |
| `dynamic-require` | Medium | `require()` with non-literal arguments |
| `dynamic-import` | Medium | `import()` with potentially dynamic paths |
| `deprecated-cipher` | Medium | Use of broken crypto (MD5, SHA1, RC4, DES) |
| `weak-crypto` | Medium | `Math.random()` for security contexts |
| `fs-dangerous-delete` | Medium | Recursive filesystem deletion |
| `fs-dangerous-write` | Medium | Dangerous filesystem writes |
| `install-scripts` | Medium | Pre/post-install scripts |

Each finding produces a structured report. Findings are deduplicated per file and per pattern, so the same `eval()` call in the same line only generates one warning.

---

## CLI reference

### `ara install`

Install all dependencies from `package.json`. Ara reads it natively as the primary source of truth, optionally merging advanced configurations (like security rules) from `ara.toml`. Resolves versions, fetches tarballs, scans for security issues, and writes `ara.lock`.

```bash
ara install                    # Interactive — prompts for suspicious packages
ara install --non-interactive  # Silent — useful for CI
```

Works with existing npm projects out of the box — no migration step needed.

### `ara add` (Direct package install)

You can install packages directly by spec and save them to the manifest. Ara resolves the spec, downloads and extracts the package, and writes the updated `package.json` and lockfile — all in one step.

```bash
ara add react                    # latest from npm registry
ara add react@18.2.0             # exact version
ara add zod@^3                   # range (resolved to latest matching)
ara add --save-dev eslint        # save as dev dependency
ara add --range=caret zod        # save with ^ prefix
ara add react zod typescript     # multiple packages at once
```

Note: `ara install <package>` is aliased to `ara add <package>`.

Supported spec formats:

| Format | Example | Target |
|--------|---------|--------|
| `name` | `react` | npm registry (latest) |
| `name@version` | `react@18.2.0` | npm registry (exact) |
| `name@^range` | `zod@^3.23.0` | npm registry (range) |
| `@scope/name` | `@angular/core` | npm scoped package |
| `@scope/name@version` | `@angular/core@17.0.0` | npm scoped exact |
| `user/repo` | `facebook/react` | GitHub shorthand |
| `user/repo#ref` | `facebook/react#v18.0.0` | GitHub with tag/branch |
| Git URL | `https://github.com/user/repo.git` | Git repository |
| Git URL + ref | `https://github.com/user/repo.git#v1.0` | Git with tag/branch |
| Tarball URL | `https://example.com/pkg.tgz` | Direct tarball |
| Local tarball | `./downloads/pkg.tar.gz` | Local file |

Flags:

| Flag | Description |
|------|-------------|
| `--save-dev` | Save as dev dependency |
| `--save-peer` | Save as peer dependency |
| `--save-optional` | Save as optional dependency |
| `--range` | Version range strategy: `exact` (default), `caret` (^), `patch` (~) |
| `--force` | Re-download even if cached |
| `--refresh` | Re-fetch for mutable references (branches, tags) |
| `--offline` | Fail if package is not in cache |

### `ara x <package> [args...]` (Execute packages)

Execute a package binary on the fly without modifying your project's manifest, similar to `npx` or `pnpm dlx`. Ara downloads the package to an isolated temporary directory, resolves its dependencies, finds its binary, and executes it securely. The temporary environment is automatically cleaned up after execution.

```bash
ara x create-next-app my-app         # Run create-next-app
ara x shadcn init --preset bdvw9FeS  # Pass arguments to the package
```

By default, the command runs under the `open` sandbox profile so it can interact with your local filesystem and the network (necessary for scaffolding tools like `create-next-app`).

### `ara run <script> --profile <profile>`

Run a script defined in `ara.toml` under a sandbox profile.

```bash
ara run build
ara run test --profile restricted
ara run build --profile hermetic
```

Profiles: `open` (or `runtime`), `restricted`, `hermetic`, `custom`.

### `ara analyze [path]`

Analyze a package (defaults to current directory) for security patterns and print findings to stdout.

```bash
ara analyze
ara analyze ./some-package
```

### `ara audit [path]`

Full security audit — same as `analyze` but with an extended report format.

### `ara gc`

Garbage-collect the content-addressable store (remove orphaned objects).

### Coming soon

- `ara build` — execute build steps with sandboxing and output hashing
- `ara publish` — publish packages with signature verification
- `ara trust <package>` — mark a package as trusted to skip future prompts

---

## Manifest format

Ara embraces a **hybrid manifest architecture** to maximize compatibility with the broader JavaScript ecosystem while maintaining its advanced capabilities:

- **`package.json`**: The absolute source of truth for your project identity, dependencies, devDependencies, scripts, and workspaces.
- **`ara.toml`**: An optional configuration file used strictly for Ara's advanced features, like security policies and hermetic build profiles.

### `package.json` (Primary Manifest)

Ara reads `package.json` natively. When you run `ara install react`, Ara will write the dependency directly to your `package.json`, just like npm or Yarn. All standard fields are mapped and respected:

- `name` and `version`
- `dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies` — including `workspace:` protocol versions (e.g. `"lib-a": "workspace:^"`)
- `scripts`
- `workspaces` — glob patterns listing workspace member directories

### `ara.toml` (Advanced Configuration)

You only need an `ara.toml` if you want to enforce security thresholds or sandbox profiles. It does not store dependencies or scripts.

#### Security options

```toml
[security]
risk_threshold = "high"       # Only warn on High+ findings (low, medium, high, critical)
require_review = true          # Always prompt for review
```

#### Build options

```toml
[build]
hermetic = true               # Run build in hermetic sandbox
offline_first = true           # Prefer local cache over network
```

---

## The lockfile (`ara.lock`)

Ara generates a `ara.lock` after every successful install. It contains the full resolved dependency graph with hashes, sources, and versions — committed to your repository for reproducibility.

```toml
version = 1

[graph]
resolver = "mvs"
generated_at = "2025-06-03T12:00:00Z"
graph_hash = "sha256:abc123..."

[[package]]
name = "zod"
version = "3.23.8"
source = "npm"
package_hash = "sha256:def456..."
integrity = "sha256:ghi789..."
dependencies = []
```

---

## Inspirations

Ara draws from projects that got it right:

- **Go Modules** for MVS and deterministic resolution
- **pnpm** for content-addressed storage and disk efficiency
- **Nix** for reproducibility and hermetic builds
- **Cargo** for its manifest format and developer experience
- **npm** for, well, being the ecosystem we all know

But Ara is not a clone of any of them. It's an experiment in what a package manager looks like when security, determinism, and developer experience are equal citizens from day one.

---

## Project status

Ara is in early development. Core install (manifest-based and direct spec), run, and analysis features work. Direct install supports npm, GitHub, git, and tarball targets with caching and security scanning. Build, publish, SBOM generation, and LAN distribution are on the roadmap.

---

## Limitations

Ara is honest about what it cannot do yet — or cannot do well.

### Linux-only sandboxing

The seccomp-BPF sandbox is Linux-specific and only supports x86_64 syscall numbers. On macOS or Windows, `ara run` degrades to running the script without restrictions. Cross-platform sandbox profiles depend on platform-specific primitives (or a VM layer) that have not been implemented.

### npm ecosystem compatibility gap

Ara uses `package.json` as the primary source of truth for dependencies and scripts, so existing npm projects work out of the box with zero migration effort. However, there is no `package-lock.json`, `yarn.lock`, or `pnpm-lock.yaml` import — Ara resolves the tree from scratch and uses its own `ara.lock` format.

### No private registry support

Ara can fetch from public npm registries but does not handle authentication tokens, `.npmrc` credentials, or scoped private packages. If your workflow depends on a private registry, Ara will not work for you today.

### Sequential downloads

Packages are fetched one at a time during install. There is no concurrent download queue, no connection pooling, and no registry-side caching. Large projects with many dependencies install noticeably slower than npm or pnpm.

### Limited constraint semantics

Ara's version constraint parser handles the common cases (`^`, `~`, `>=`, `<=`, exact, wildcard) but does not support complex ranges like `>=1.0.0 <2.0.0`, `||` combinators, or prerelease-aware resolution. The MVS resolver picks the lowest matching version, which is correct for determinism but may disagree with npm's behavior on overlapping ranges.

### No publish or distribution

Ara cannot publish packages to npm, GitHub Packages, or any other registry. Publishing exists as a stub command only. This also means Ara cannot sign packages, generate provenance statements, or verify signatures from other publishers.

### No lifecycle scripts

Unlike npm's `preinstall`, `postinstall`, `prepare`, and friends, Ara does not run any package lifecycle scripts. This is intentional from a security perspective, but it means packages that rely on install-time code generation or native compilation will not work out of the box.

### Single-binary, no library API

Ara is compiled as a single binary with no stable Rust library interface. If you want to embed Ara's resolver or analyzer in your own tool, you would have to fork or shell out. There is no Cargo-like lib.rs separation.

### x86_64 focus

The content store and hash formats are architecture-agnostic, but the sandbox syscall tables are written for x86_64 Linux only. ARM64, RISC-V, and other architectures require their own syscall number tables, which do not exist yet.


> These limitations may resolve themselves soon, don't worry, test the app, give feedback, contribute with issues and pull requests.

---

License: MIT
