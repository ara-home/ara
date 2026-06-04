//! Sandboxed command execution with Linux seccomp-BPF filters.
//! Supports three profiles: Hermetic (minimal syscalls), Restricted (safe syscalls),
//! and Open (no restrictions).

use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::sandbox::profiles::{Profile, SandboxConfig};

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

    pub fn execute(&self, command: &str) -> Result<(), ExecutorError> {
        let profile = self.config.profile;

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(move || apply_seccomp(profile));
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

const SECCOMP_RET_KILL: u32 = 0x0000_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

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
#[cfg(target_arch = "x86_64")]
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
    pub const PRCTL: i32 = 157;
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
}

// Syscall whitelists per profile
#[cfg(target_os = "linux")]
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
    sys::PRCTL,
];

#[cfg(target_os = "linux")]
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
    sys::PRCTL,
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

#[allow(clippy::cast_sign_loss)]
fn build_seccomp_filter(allowed: &[i32]) -> Vec<sock_filter> {
    let mut filters = Vec::with_capacity(allowed.len() + 3);

    // Load syscall number: A = seccomp_data.nr (offset 0)
    // BPF_LD | BPF_W | BPF_ABS with k=0 loads the arch field
    // Actually, for seccomp_data: offset 0 is nr (syscall number)
    // The BPF_LD instruction loads into A (accumulator)
    filters.push(sock_filter {
        code: BPF_LD | BPF_W | BPF_ABS,
        jt: 0,
        jf: 0,
        k: 0, // offset 0 = syscall number
    });

    // For each allowed syscall, add a JEQ check
    // Format: if A == sysno, jump to ALLOW (skip 0, so next instruction is ALLOW)
    //         otherwise, continue to next check
    for &sysno in allowed {
        filters.push(sock_filter {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 0, // if equal, skip 0 instructions → execute next (which is ALLOW)
            jf: 0, // if not equal, skip 0 → continue to next check
            k: sysno as u32,
        });

        // ALLOW: return SECCOMP_RET_ALLOW
        filters.push(sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        });
    }

    // Default: KILL
    filters.push(sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k: SECCOMP_RET_KILL,
    });

    filters
}

#[cfg(target_os = "linux")]
#[allow(clippy::cast_possible_truncation)]
fn apply_seccomp(profile: Profile) -> Result<(), std::io::Error> {
    let allowed = match profile {
        Profile::Hermetic => HERMETIC_SYSCALLS,
        Profile::Restricted => RESTRICTED_SYSCALLS,
        Profile::Open | Profile::Custom => return Ok(()),
    };

    let filters = build_seccomp_filter(allowed);

    let prog = sock_fprog {
        len: filters.len() as u16,
        filter: filters.as_ptr(),
    };

    let ret = unsafe { libc::prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &raw const prog) };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_seccomp(_profile: Profile) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_build_filter_hermetic() {
        let filters = build_seccomp_filter(HERMETIC_SYSCALLS);
        assert!(filters.len() > 3);
        // First instruction should be LD
        assert_eq!(filters[0].code, BPF_LD | BPF_W | BPF_ABS);
        // Last instruction should be RET KILL
        assert_eq!(filters.last().unwrap().code, BPF_RET | BPF_K);
        assert_eq!(filters.last().unwrap().k, SECCOMP_RET_KILL);
    }
}
