//! Sandboxed command execution with Linux seccomp-BPF filters.
//! Supports three profiles: Hermetic (minimal syscalls), Restricted (safe syscalls),
//! and Open (no restrictions).

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::profiles::{Profile, SandboxConfig};

#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Executor {
    pub config: SandboxConfig,
}

impl Executor {
    #[must_use]
    pub const fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    /// Execute a shell command string via `sh -c`.
    /// Used for manifest scripts (e.g. `"node build.js && echo done"`).
    pub fn execute(
        &self,
        command: &str,
        env: Option<HashMap<String, String>>,
    ) -> Result<(), ExecutorError> {
        warn_no_sandbox(self.config.profile);

        let profile = self.config.profile;
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        if let Some(env) = env {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        // Build the BPF filter in the parent process (heap allocation is
        // safe here).  The pre-built filter is passed as a reference to the
        // async-signal-safe pre_exec closure so the child only calls prctl().
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(filter) = build_seccomp_filter_for_profile(profile) {
            unsafe {
                cmd.pre_exec(move || apply_seccomp_filter(&filter));
            }
        }

        let mut child = cmd.spawn()?;
        let status = child.wait()?;

        if !status.success() {
            return Err(ExecutorError::Io(std::io::Error::other(format!(
                "command exited with: {status}"
            ))));
        }

        Ok(())
    }

    /// Execute a binary directly with individual arguments (no shell).
    /// Used for `ara x` where arguments come from user input.
    pub fn execute_program(
        &self,
        program: &Path,
        args: &[String],
        env: Option<HashMap<String, String>>,
    ) -> Result<(), ExecutorError> {
        warn_no_sandbox(self.config.profile);

        let profile = self.config.profile;
        let mut cmd = Command::new(program);
        cmd.args(args);

        if let Some(env) = env {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        // Build the BPF filter in the parent process (heap allocation is
        // safe here).  The pre-built filter is passed as a reference to the
        // async-signal-safe pre_exec closure so the child only calls prctl().
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(filter) = build_seccomp_filter_for_profile(profile) {
            unsafe {
                cmd.pre_exec(move || apply_seccomp_filter(&filter));
            }
        }

        let mut child = cmd.spawn()?;
        let status = child.wait()?;

        if !status.success() {
            return Err(ExecutorError::Io(std::io::Error::other(format!(
                "command exited with: {status}"
            ))));
        }

        Ok(())
    }
}

/// Emit a warning when the sandbox is not active.
fn warn_no_sandbox(profile: Profile) {
    #[cfg(target_os = "linux")]
    {
        if profile == Profile::Open || profile == Profile::Custom {
            eprintln!(
                "  warning: running with {} profile — no syscall restrictions applied",
                profile
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!(
            "  warning: sandbox is not supported on this platform (Linux seccomp only); \
             running without restrictions"
        );
    }
}

// ---------------------------------------------------------------------------
// seccomp — Linux BPF sandboxing
// ---------------------------------------------------------------------------

// BPF instruction constants for seccomp
const BPF_LD: u16 = 0x00;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;

const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

// Audit architecture for x86_64 (used in seccomp arch validation)
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;

// seccomp constants
const PR_SET_SECCOMP: i32 = 22;
const SECCOMP_MODE_FILTER: i32 = 2;

#[repr(C)]
struct sock_filter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[repr(C)]
struct sock_fprog {
    len: u16,
    filter: *const sock_filter,
}

// Linux x86_64 syscall numbers
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod sys {
    pub const READ: i32 = 0;
    pub const WRITE: i32 = 1;
    pub const OPEN: i32 = 2;
    pub const CLOSE: i32 = 3;
    pub const STAT: i32 = 4;
    pub const FSTAT: i32 = 5;
    pub const LSTAT: i32 = 6;
    pub const POLL: i32 = 7;
    pub const LSEEK: i32 = 8;
    pub const MMAP: i32 = 9;
    pub const MPROTECT: i32 = 10;
    pub const MUNMAP: i32 = 11;
    pub const BRK: i32 = 12;
    pub const SIGACTION: i32 = 13;
    pub const SIGPROCMASK: i32 = 14;
    pub const RT_SIGRETURN: i32 = 15;
    pub const IOCTL: i32 = 16;
    pub const PREAD64: i32 = 17;
    pub const PWRITE64: i32 = 18;
    pub const READV: i32 = 19;
    pub const WRITEV: i32 = 20;
    pub const ACCESS: i32 = 21;
    pub const PIPE: i32 = 22;
    pub const SELECT: i32 = 23;
    pub const SCHED_YIELD: i32 = 24;
    pub const MADVISE: i32 = 28;
    pub const DUP: i32 = 32;
    pub const DUP2: i32 = 33;
    pub const NANOSLEEP: i32 = 35;
    pub const GETITIMER: i32 = 36;
    pub const SETITIMER: i32 = 38;
    pub const GETPID: i32 = 39;
    pub const SENDFILE: i32 = 40;
    pub const EXIT: i32 = 60;
    pub const UNAME: i32 = 63;
    pub const FCNTL: i32 = 72;
    pub const FLOCK: i32 = 73;
    pub const FSYNC: i32 = 74;
    pub const GETDENTS: i32 = 78;
    pub const GETCWD: i32 = 79;
    pub const CHDIR: i32 = 80;
    pub const FCHDIR: i32 = 81;
    pub const MKDIR: i32 = 83;
    pub const READLINK: i32 = 89;
    pub const UMASK: i32 = 95;
    pub const GETTIMEOFDAY: i32 = 96;
    pub const SYSINFO: i32 = 99;
    pub const GETUID: i32 = 102;
    pub const GETGID: i32 = 104;
    pub const GETEUID: i32 = 107;
    pub const GETEGID: i32 = 108;
    pub const GETPPID: i32 = 110;
    pub const RT_SIGPENDING: i32 = 127;
    pub const RT_SIGTIMEDWAIT: i32 = 128;
    pub const PERSONALITY: i32 = 135;
    pub const SYSFS: i32 = 139;
    pub const GETRLIMIT: i32 = 161;
    pub const SETRLIMIT: i32 = 160;
    pub const FUTEX: i32 = 202;
    pub const CLOCK_GETTIME: i32 = 228;
    pub const CLOCK_NANOSLEEP: i32 = 230;
    pub const EXIT_GROUP: i32 = 231;
    pub const EPOLL_WAIT: i32 = 232;
    pub const EPOLL_CTL: i32 = 233;
    pub const OPENAT: i32 = 257;
    pub const NEWFSTATAT: i32 = 262;
    pub const READLINKAT: i32 = 267;
    pub const EVENTFD: i32 = 284;
    pub const PIPE2: i32 = 293;
    pub const MKDIRAT: i32 = 258;
    pub const GETRANDOM: i32 = 318;
    pub const EPOLL_CREATE: i32 = 213;
    pub const CLONE: i32 = 56;
    pub const FORK: i32 = 57;
    pub const VFORK: i32 = 58;
}

// Syscall whitelists per profile
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const HERMETIC_SYSCALLS: &[i32] = &[
    sys::READ,
    sys::WRITE,
    sys::CLOSE,
    sys::MMAP,
    sys::MUNMAP,
    sys::MPROTECT,
    sys::BRK,
    sys::EXIT,
    sys::EXIT_GROUP,
    sys::RT_SIGRETURN,
    sys::SIGACTION,
    sys::SIGPROCMASK,
    sys::NANOSLEEP,
    sys::CLOCK_NANOSLEEP,
    sys::CLOCK_GETTIME,
    sys::GETTIMEOFDAY,
    sys::FUTEX,
    sys::LSEEK,
    sys::FSTAT,
    sys::NEWFSTATAT,
    sys::GETRANDOM,
    sys::FORK,
    sys::CLONE,
    sys::VFORK,
];

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const RESTRICTED_SYSCALLS: &[i32] = &[
    sys::READ,
    sys::WRITE,
    sys::OPEN,
    sys::OPENAT,
    sys::CLOSE,
    sys::STAT,
    sys::FSTAT,
    sys::NEWFSTATAT,
    sys::LSTAT,
    sys::LSEEK,
    sys::MMAP,
    sys::MUNMAP,
    sys::MPROTECT,
    sys::BRK,
    sys::EXIT,
    sys::EXIT_GROUP,
    sys::RT_SIGRETURN,
    sys::SIGACTION,
    sys::SIGPROCMASK,
    sys::NANOSLEEP,
    sys::CLOCK_NANOSLEEP,
    sys::CLOCK_GETTIME,
    sys::GETTIMEOFDAY,
    sys::FUTEX,
    sys::PREAD64,
    sys::PWRITE64,
    sys::READV,
    sys::WRITEV,
    sys::ACCESS,
    sys::GETDENTS,
    sys::GETCWD,
    sys::CHDIR,
    sys::FCHDIR,
    sys::FCNTL,
    sys::FLOCK,
    sys::FSYNC,
    sys::DUP,
    sys::DUP2,
    sys::PIPE,
    sys::PIPE2,
    sys::SELECT,
    sys::POLL,
    sys::SENDFILE,
    sys::GETPID,
    sys::GETPPID,
    sys::GETUID,
    sys::GETEUID,
    sys::GETGID,
    sys::GETEGID,
    sys::UMASK,
    sys::UNAME,
    sys::SYSINFO,
    sys::GETRANDOM,
    sys::FORK,
    sys::CLONE,
    sys::VFORK,
    sys::READLINK,
    sys::READLINKAT,
    sys::MKDIR,
    sys::MKDIRAT,
    sys::MADVISE,
    sys::IOCTL,
    sys::SETITIMER,
    sys::GETITIMER,
    sys::RT_SIGPENDING,
    sys::RT_SIGTIMEDWAIT,
    sys::GETRLIMIT,
    sys::SETRLIMIT,
    sys::SCHED_YIELD,
    sys::EVENTFD,
    sys::EPOLL_CREATE,
    sys::EPOLL_CTL,
    sys::EPOLL_WAIT,
    sys::SYSFS,
    sys::PERSONALITY,
];

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[allow(clippy::cast_sign_loss)]
fn build_seccomp_filter(allowed: &[i32]) -> Vec<sock_filter> {
    let mut filters = Vec::with_capacity(allowed.len() * 2 + 5);

    // 1. Validate architecture (prevent personality switch bypass on x86)
    //    Load the architecture field from seccomp_data (offset 4).
    //    If not x86_64, kill the process immediately.
    filters.push(sock_filter {
        code: BPF_LD | BPF_W | BPF_ABS,
        jt: 0,
        jf: 0,
        k: 4, // offset 4 = architecture field
    });
    // If NOT x86_64 -> KILL; if x86_64 -> skip KILL and continue
    filters.push(sock_filter {
        code: BPF_JMP | BPF_JEQ | BPF_K,
        jt: 1, // if equal (x86_64), skip 1 instruction (skip KILL)
        jf: 0, // if NOT equal, execute next instruction (KILL)
        k: AUDIT_ARCH_X86_64,
    });
    filters.push(sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k: SECCOMP_RET_KILL_PROCESS,
    });

    // 2. Load syscall number: A = seccomp_data.nr (offset 0)
    filters.push(sock_filter {
        code: BPF_LD | BPF_W | BPF_ABS,
        jt: 0,
        jf: 0,
        k: 0,
    });

    // 3. For each allowed syscall, add a JEQ check
    //    If A == sysno -> ALLOW; otherwise -> check next syscall
    for &sysno in allowed {
        filters.push(sock_filter {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 0, // if equal, skip 0 -> execute ALLOW
            jf: 1, // if NOT equal, skip 1 -> skip ALLOW, go to next check
            k: sysno as u32,
        });
        filters.push(sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        });
    }

    // 4. Default: kill the entire process (not just the thread)
    filters.push(sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k: SECCOMP_RET_KILL_PROCESS,
    });

    filters
}

/// Build a seccomp filter for the given profile, or return `None` if the
/// profile does not use seccomp (Open / Custom).
///
/// Called in the **parent** process where heap allocation is safe.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn build_seccomp_filter_for_profile(profile: Profile) -> Option<Vec<sock_filter>> {
    match profile {
        Profile::Hermetic => Some(build_seccomp_filter(HERMETIC_SYSCALLS)),
        Profile::Restricted => Some(build_seccomp_filter(RESTRICTED_SYSCALLS)),
        Profile::Open | Profile::Custom => None,
    }
}

/// Stub for non-Linux or non-x86_64 platforms: seccomp is not available.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn build_seccomp_filter_for_profile(_profile: Profile) -> Option<Vec<sock_filter>> {
    None
}

/// Apply a pre-built seccomp-BPF filter via `prctl`.
///
/// Must only be called in the child process after `fork()` (e.g. via `pre_exec`).
/// **Async-signal-safe**: performs no heap allocation.
///
/// Automatically sets `PR_SET_NO_NEW_PRIVS` before installing the filter to
/// prevent the child from gaining new privileges (setuid binaries) and to
/// block installation of a more permissive seccomp filter.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn apply_seccomp_filter(filter: &[sock_filter]) -> Result<(), std::io::Error> {
    // Prevent privilege escalation and further seccomp modifications.
    let ret = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let prog = sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr(),
    };

    let ret = libc::prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &raw const prog);
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn test_build_filter_hermetic() {
        let filters = build_seccomp_filter(HERMETIC_SYSCALLS);
        // Structure: arch LD (idx 0) + arch JEQ (idx 1) + arch KILL (idx 2)
        //            + syscall LD (idx 3) + N*(JEQ + ALLOW) + final KILL
        assert!(filters.len() > 6);
        // First instruction should be LD for architecture (offset 4)
        assert_eq!(filters[0].code, BPF_LD | BPF_W | BPF_ABS);
        assert_eq!(
            filters[0].k, 4,
            "first load should read arch field (offset 4)"
        );
        // Second instruction should be JEQ for arch comparison
        assert_eq!(filters[1].code, BPF_JMP | BPF_JEQ | BPF_K);
        assert_eq!(filters[1].k, AUDIT_ARCH_X86_64);
        // Third instruction should be RET KILL for wrong arch
        assert_eq!(filters[2].code, BPF_RET | BPF_K);
        assert_eq!(filters[2].k, SECCOMP_RET_KILL_PROCESS);
        // Fourth instruction should be LD for syscall number (offset 0)
        assert_eq!(filters[3].code, BPF_LD | BPF_W | BPF_ABS);
        assert_eq!(
            filters[3].k, 0,
            "fourth load should read syscall nr (offset 0)"
        );
        // Last instruction should be RET KILL_PROCESS
        assert_eq!(filters.last().unwrap().code, BPF_RET | BPF_K);
        assert_eq!(filters.last().unwrap().k, SECCOMP_RET_KILL_PROCESS);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn test_build_filter_jf_not_zero() {
        // Verify that the jf field is 1 (not 0), meaning non-matching
        // syscalls correctly skip the ALLOW and fall through to the next check.
        let filters = build_seccomp_filter(&[sys::WRITE]);
        // Indices: 0=arch LD, 1=arch JEQ, 2=arch KILL, 3=syscall LD
        //          4=JEQ for WRITE, 5=RET ALLOW, 6=RET KILL_PROCESS
        assert_eq!(
            filters[4].jt, 0,
            "jt should be 0 (matched syscall -> ALLOW)"
        );
        assert_eq!(
            filters[4].jf, 1,
            "jf should be 1 (non-match -> skip ALLOW, go to next check)"
        );
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn test_build_filter_for_profile_open_is_none() {
        assert!(build_seccomp_filter_for_profile(Profile::Open).is_none());
        assert!(build_seccomp_filter_for_profile(Profile::Custom).is_none());
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn test_build_filter_for_profile_hermetic_is_some() {
        assert!(build_seccomp_filter_for_profile(Profile::Hermetic).is_some());
        assert!(build_seccomp_filter_for_profile(Profile::Restricted).is_some());
    }
}
