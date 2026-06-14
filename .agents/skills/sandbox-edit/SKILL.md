---
name: sandbox-edit
description: Modify the seccomp-BPF sandbox in ara-sandbox. HIGH RISK — only crate with unsafe code. Requires Linux x86_64 testing and cross-arch awareness.
license: MIT
compatibility: opencode
---

## Steps

1. **Identify the scope of change**:
   - Syscall table? → `executor.rs` constants (e.g. `SYS_read`, `SYS_write`, etc.)
   - Profile? → `profiles.rs` profile presets or `SandboxConfig`
   - Executor logic? → `executor.rs` execute functions or BPF filter building
2. **For syscall table changes**:
   - Update the `#[cfg(target_arch = "x86_64")]` syscall number constants
   - Ensure the new syscall is added to the correct profile whitelist (`HERMETIC_SYSCALLS` or `RESTRICTED_SYSCALLS`)
   - All syscalls are x86_64-specific — add `#[cfg]` guards if supporting other archs
3. **For profile changes**:
   - `Profile::Hermetic`: ~22 syscalls (read, write, mmap, futex, clock, etc.) — minimal set for deterministic builds
   - `Profile::Restricted`: ~80 syscalls — safe syscalls, read-only fs, no network
   - `Profile::Open`: no seccomp filter applied
   - Keep the profile restrictions meaningful — hermetic should actually prevent network and non-determinism
4. **For executor changes**:
   - The `unsafe` block wraps `libc::prctl(PR_SET_SECCOMP, ...)` in `pre_exec`
   - The `pre_exec` closure runs in the child process after `fork()` but before `exec()`
   - Avoid capturing non-trivial state in `pre_exec` closures
5. **Verify**: `cargo test -p ara-sandbox`
6. **Cross-arch check**: search for `#[cfg(target_arch = "x86_64")]` to ensure non-x86_64 platforms degrade gracefully (sandbox is a no-op with warning on other platforms)

## Critical rules

- ❌ Never add `unsafe` outside this crate — this is the only crate allowed
- ❌ Never remove `#[cfg(target_arch = "x86_64")]` guards without adding equivalent support for other archs
- ✅ Always run `cargo test -p ara-sandbox` and `cargo clippy -p ara-sandbox -- -D warnings` after changes
- ⚠️ Adding new syscalls to Hermetic profile breaks determinism guarantees — justify in the description

## When to use

Use when modifying the sandbox executor, syscall tables, or execution profiles. Load this skill whenever `ara-sandbox` source files are being edited.
