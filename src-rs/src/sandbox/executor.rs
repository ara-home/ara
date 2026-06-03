use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::sandbox::profiles::{Profile, SandboxConfig};

#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("seccomp error: {0}")]
    Seccomp(String),
}

pub struct Executor {
    pub config: SandboxConfig,
}

impl Executor {
    #[must_use]
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    pub fn execute(&self, command: &str) -> Result<(), ExecutorError> {
        let profile = self.config.profile;

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        // Install seccomp filter in the child process before exec
        unsafe {
            cmd.pre_exec(move || apply_seccomp(profile));
        }

        let mut child = cmd.spawn()?;
        let status = child.wait()?;

        if !status.success() {
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("command exited with: {status}"),
            )));
        }

        Ok(())
    }

    pub fn dry_run(&self, command: &str) {
        eprintln!("[sandbox] would execute: {command}");
        eprintln!("[sandbox] profile: {:?}", self.config.profile);
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
    #![allow(dead_code)]
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
    pub const MREMAP: i32 = 25;
    pub const MSYNC: i32 = 26;
    pub const MINCORE: i32 = 27;
    pub const MADVISE: i32 = 28;
    pub const SHMGET: i32 = 29;
    pub const SHMAT: i32 = 30;
    pub const SHMCTL: i32 = 31;
    pub const DUP: i32 = 32;
    pub const DUP2: i32 = 33;
    pub const PAUSE: i32 = 34;
    pub const NANOSLEEP: i32 = 35;
    pub const GETITIMER: i32 = 36;
    pub const ALARM: i32 = 37;
    pub const SETITIMER: i32 = 38;
    pub const GETPID: i32 = 39;
    pub const SENDFILE: i32 = 40;
    pub const SOCKET: i32 = 41;
    pub const CONNECT: i32 = 42;
    pub const ACCEPT: i32 = 43;
    pub const SENDTO: i32 = 44;
    pub const RECVFROM: i32 = 45;
    pub const SENDMSG: i32 = 46;
    pub const RECVMSG: i32 = 47;
    pub const SHUTDOWN: i32 = 48;
    pub const BIND: i32 = 49;
    pub const LISTEN: i32 = 50;
    pub const GETSOCKNAME: i32 = 51;
    pub const GETPEERNAME: i32 = 52;
    pub const SOCKETPAIR: i32 = 53;
    pub const SETSOCKOPT: i32 = 54;
    pub const GETSOCKOPT: i32 = 55;
    pub const CLONE: i32 = 56;
    pub const FORK: i32 = 57;
    pub const VFORK: i32 = 58;
    pub const EXECVE: i32 = 59;
    pub const EXIT: i32 = 60;
    pub const WAIT4: i32 = 61;
    pub const KILL: i32 = 62;
    pub const UNAME: i32 = 63;
    pub const SEMGET: i32 = 64;
    pub const SEMOP: i32 = 65;
    pub const SEMCTL: i32 = 66;
    pub const SHMDT: i32 = 67;
    pub const MSGGET: i32 = 68;
    pub const MSGSND: i32 = 69;
    pub const MSGRCV: i32 = 70;
    pub const MSGCTL: i32 = 71;
    pub const FCNTL: i32 = 72;
    pub const FLOCK: i32 = 73;
    pub const FSYNC: i32 = 74;
    pub const FDATASYNC: i32 = 75;
    pub const TRUNCATE: i32 = 76;
    pub const FTRUNCATE: i32 = 77;
    pub const GETDENTS: i32 = 78;
    pub const GETCWD: i32 = 79;
    pub const CHDIR: i32 = 80;
    pub const FCHDIR: i32 = 81;
    pub const RENAME: i32 = 82;
    pub const MKDIR: i32 = 83;
    pub const RMDIR: i32 = 84;
    pub const CREAT: i32 = 85;
    pub const LINK: i32 = 86;
    pub const UNLINK: i32 = 87;
    pub const SYMLINK: i32 = 88;
    pub const READLINK: i32 = 89;
    pub const CHMOD: i32 = 90;
    pub const FCHMOD: i32 = 91;
    pub const CHOWN: i32 = 92;
    pub const FCHOWN: i32 = 93;
    pub const LCHOWN: i32 = 94;
    pub const UMASK: i32 = 95;
    pub const GETTIMEOFDAY: i32 = 96;
    pub const GETRUSAGE: i32 = 97;
    pub const SYSINFO: i32 = 99;
    pub const TIMES: i32 = 100;
    pub const PTRACE: i32 = 101;
    pub const GETUID: i32 = 102;
    pub const SYSLOG: i32 = 103;
    pub const GETGID: i32 = 104;
    pub const SETUID: i32 = 105;
    pub const SETGID: i32 = 106;
    pub const GETEUID: i32 = 107;
    pub const GETEGID: i32 = 108;
    pub const SETPGID: i32 = 109;
    pub const GETPPID: i32 = 110;
    pub const GETPGRP: i32 = 111;
    pub const SETSID: i32 = 112;
    pub const SETREUID: i32 = 113;
    pub const SETREGID: i32 = 114;
    pub const GETGROUPS: i32 = 115;
    pub const SETGROUPS: i32 = 116;
    pub const SETRESUID: i32 = 117;
    pub const GETRESUID: i32 = 118;
    pub const SETRESGID: i32 = 119;
    pub const GETRESGID: i32 = 120;
    pub const GETPGID: i32 = 121;
    pub const SETFSUID: i32 = 122;
    pub const SETFSGID: i32 = 123;
    pub const GETSID: i32 = 124;
    pub const CAPGET: i32 = 125;
    pub const CAPSET: i32 = 126;
    pub const RT_SIGPENDING: i32 = 127;
    pub const RT_SIGTIMEDWAIT: i32 = 128;
    pub const RT_SIGQUEUEINFO: i32 = 129;
    pub const RT_SIGSUSPEND: i32 = 130;
    pub const SIGALTSTACK: i32 = 131;
    pub const UTIME: i32 = 132;
    pub const MKNOD: i32 = 133;
    pub const USELIB: i32 = 134;
    pub const PERSONALITY: i32 = 135;
    pub const USTAT: i32 = 136;
    pub const STATFS: i32 = 137;
    pub const FSTATFS: i32 = 138;
    pub const SYSFS: i32 = 139;
    pub const GETPRIORITY: i32 = 140;
    pub const SETPRIORITY: i32 = 141;
    pub const SCHED_SETPARAM: i32 = 142;
    pub const SCHED_GETPARAM: i32 = 143;
    pub const SCHED_SETSCHEDULER: i32 = 144;
    pub const SCHED_GETSCHEDULER: i32 = 145;
    pub const SCHED_GET_PRIORITY_MAX: i32 = 146;
    pub const SCHED_GET_PRIORITY_MIN: i32 = 147;
    pub const SCHED_RR_GET_INTERVAL: i32 = 148;
    pub const MLOCK: i32 = 149;
    pub const MUNLOCK: i32 = 150;
    pub const MLOCKALL: i32 = 151;
    pub const MUNLOCKALL: i32 = 152;
    pub const VHANGUP: i32 = 153;
    pub const MODIFY_LDT: i32 = 154;
    pub const PIVOT_ROOT: i32 = 155;
    pub const _SYSCTL: i32 = 156;
    pub const PRCTL: i32 = 157;
    pub const ARCH_PRCTL: i32 = 158;
    pub const ADJTIMEX: i32 = 159;
    pub const SETRLIMIT: i32 = 160;
    pub const GETRLIMIT: i32 = 161;
    pub const GETRANDOM: i32 = 318;
    pub const CLOCK_GETTIME: i32 = 228;
    pub const EXIT_GROUP: i32 = 231;
    pub const NEWFSTATAT: i32 = 262;
    pub const OPENAT: i32 = 257;
    pub const MKDIRAT: i32 = 258;
    pub const READLINKAT: i32 = 267;
    pub const FSTATAT: i32 = 262;
    pub const STATX: i32 = 332;
    pub const PIPE2: i32 = 293;
    pub const DUP3: i32 = 292;
    pub const EVENTFD: i32 = 284;
    pub const EPOLL_CREATE: i32 = 213;
    pub const EPOLL_CTL: i32 = 233;
    pub const EPOLL_WAIT: i32 = 232;
    pub const CLOCK_NANOSLEEP: i32 = 230;
    pub const FUTEX: i32 = 202;
    pub const NEWSELECT: i32 = 23;
    pub const WRITEV_P: i32 = 20;
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
            jt: 0,  // if equal, skip 0 instructions → execute next (which is ALLOW)
            jf: 0,  // if not equal, skip 0 → continue to next check
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

    let ret = unsafe {
        libc::prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &prog as *const sock_fprog,
        )
    };

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
    use super::*;

    #[test]
    fn test_dry_run() {
        let config = SandboxConfig::for_profile(Profile::Restricted);
        let ex = Executor::new(config);
        // just ensure it doesn't crash
        ex.dry_run("echo hello");
    }

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
