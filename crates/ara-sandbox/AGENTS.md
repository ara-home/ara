# ara-sandbox

Seccomp-BPF sandboxed execution for running scripts with restricted syscall profiles. Zero internal ara dependencies. **The only crate with `unsafe` code.**

## Modules & Public API

**`profiles.rs`**:
- `Profile` enum: `Open`, `Restricted`, `Hermetic`, `Custom`
- `SandboxConfig { profile, filesystem, network, environment, process, clock }` — `for_profile(profile)` builder
- `UnknownProfile` error
- Open profile: no restrictions; Restricted: ~80 safe syscalls, no network; Hermetic: ~22 minimal syscalls, deterministic clock
- Non-Linux: sandbox is a no-op with warning

**`executor.rs`**:
- `Executor { config }` — `Executor::new(config)`
- `execute(command, env) -> Result<()>` — run via `sh -c`
- `execute_program(program, args, env) -> Result<()>` — run binary directly
- `unsafe { libc::prctl(PR_SET_SECCOMP, ...) }` — applies BPF filter in `pre_exec` child process hook
- `ExecutorError` enum

## Conventions

- **All 3 `unsafe` blocks** are in `executor.rs` — never add `unsafe` outside this crate
- x86_64 specific syscall numbers (`#[cfg(target_arch = "x86_64")]`)
- BPF filter applied via `pre_exec` (after `fork()`, before `exec()` in child)
- `sh -c` for scripts, direct exec for binaries (`ara x`)
- Open/Custom profiles don't apply seccomp — warn on stderr
- Non-Linux: degrades to unrestricted with warning

## Test

`cargo test -p ara-sandbox`

## Dependencies

- External: `libc`, `thiserror`
- Internal: none
