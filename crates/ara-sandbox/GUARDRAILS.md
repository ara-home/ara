# GUARDRAILS.md — ara-sandbox

---

## SIGN #1: Unsafe Proliferation
**Trigger:** Adding `unsafe` outside `ara-sandbox` or adding unnecessary `unsafe` blocks within it
**Instruction:** All `unsafe` blocks in the project must live in `ara-sandbox`. Within the crate, minimize `unsafe` scope — wrap each `unsafe` block as tightly as possible. Never add `unsafe` for performance speculation.
**Reason:** This crate is the designated unsafe boundary. Containing all `unsafe` here makes auditing possible. Every `unsafe` block must be justified.
**Provenance:** Project-wide invariant. Current count: 3 `unsafe` blocks, all in `executor.rs`.

---

## SIGN #2: Architecture-Specific Syscalls
**Trigger:** Adding or modifying syscall numbers without `#[cfg(target_arch = "x86_64")]` guards, or removing existing guards
**Instruction:** All syscall numbers must be guarded by `#[cfg(target_arch = "x86_64")]`. If adding support for another architecture, add new `#[cfg]` blocks — do NOT remove the x86_64 ones. Non-Linux platforms must degrade gracefully (no-op with warning).
**Reason:** Syscall numbers are architecture-specific. x86_64 numbers do not apply to ARM64, RISC-V, or other architectures. Removing guards breaks the build on non-x86_64 targets.
**Provenance:** Architecture-specific since initial sandbox implementation.

---

## SIGN #3: Hermetic Profile Stability
**Trigger:** Adding syscalls to the `HERMETIC_SYSCALLS` whitelist
**Instruction:** Every addition to the hermetic profile must be justified in code review. Document what the syscall enables and why it's necessary for deterministic builds.
**Reason:** The hermetic profile guarantees deterministic, network-isolated, clock-fixed builds. Each added syscall expands the attack surface and potentially reduces determinism.
**Provenance:** Hermetic profile defined with ~22 syscalls in `executor.rs`.
