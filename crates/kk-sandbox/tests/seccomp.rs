use kk_sandbox::seccomp::{self, Insn, AARCH64, AF_UNIX, X86_64};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 10;
const AF_NETLINK: u32 = 16;
const AF_PACKET: u32 = 17;
const AF_VSOCK: u32 = 40;

const ALLOW: u32 = 0x7fff_0000;
const EPERM: u32 = 0x0005_0001;
const KILL: u32 = 0x8000_0000;

/// A classic-BPF interpreter for the instructions the filter uses, over a `struct seccomp_data`.
fn run(prog: &[Insn], arch: u32, nr: u32, arg0: u64) -> u32 {
    let mut data = [0u8; 64];
    data[0..4].copy_from_slice(&nr.to_le_bytes());
    data[4..8].copy_from_slice(&arch.to_le_bytes());
    data[16..24].copy_from_slice(&arg0.to_le_bytes());
    let mut acc = 0u32;
    let mut pc = 0usize;
    for _ in 0..prog.len() + 1 {
        let i = prog[pc];
        match i.code {
            0x20 => {
                let o = i.k as usize;
                acc = u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
                pc += 1;
            }
            0x15 => pc += 1 + if acc == i.k { i.jt } else { i.jf } as usize,
            0x35 => pc += 1 + if acc >= i.k { i.jt } else { i.jf } as usize,
            0x06 => return i.k,
            other => panic!("unexpected opcode {other:#x}"),
        }
    }
    panic!("the program did not finish");
}

/// What the kernel checks before it accepts a program: it ends in a return, every jump stays inside it and goes forward.
fn validate(prog: &[Insn]) {
    assert!(!prog.is_empty() && prog.len() <= 4096, "{}", prog.len());
    assert!(
        matches!(prog.last().unwrap().code, 0x06),
        "ends in a return"
    );
    for (idx, i) in prog.iter().enumerate() {
        match i.code {
            0x20 => assert!(i.k <= 60 && i.k % 4 == 0, "load offset {}", i.k),
            0x15 | 0x35 => {
                assert!(idx + 1 + (i.jt as usize) < prog.len(), "jt at {idx}");
                assert!(idx + 1 + (i.jf as usize) < prog.len(), "jf at {idx}");
            }
            0x06 => assert_eq!((i.jt, i.jf), (0, 0)),
            other => panic!("opcode {other:#x} at {idx}"),
        }
    }
}

#[test]
fn sc01_the_program_is_well_formed_and_bwrap_can_read_its_bytes() {
    let p = seccomp::program();
    validate(&p);
    let b = seccomp::bytes();
    assert_eq!(b.len(), p.len() * 8);
    assert_eq!(&b[0..2], &0x20u16.to_le_bytes(), "first instruction loads");
    assert_eq!(&b[4..8], &4u32.to_le_bytes(), "from the architecture field");
}

#[test]
fn sc02_every_denied_call_is_refused_on_both_architectures() {
    let p = seccomp::program();
    for t in [&X86_64, &AARCH64] {
        for n in t.denied {
            assert_eq!(run(&p, t.arch, n, 0), EPERM, "arch {:#x} call {n}", t.arch);
            assert_eq!(
                run(&p, t.arch, n, AF_UNIX as u64),
                EPERM,
                "arg does not matter"
            );
        }
    }
}

#[test]
fn sc03_only_unix_sockets_can_be_created() {
    let p = seccomp::program();
    for t in [&X86_64, &AARCH64] {
        for call in [t.socket, t.socketpair] {
            assert_eq!(run(&p, t.arch, call, AF_UNIX as u64), ALLOW);
            for fam in [
                0,
                AF_INET,
                AF_INET6,
                AF_NETLINK,
                AF_PACKET,
                AF_VSOCK,
                3,
                41,
                255,
                u32::MAX,
            ] {
                assert_eq!(run(&p, t.arch, call, fam as u64), EPERM, "family {fam}");
            }
            // the kernel reads the family as a 32-bit int, so the program compares the low half
            assert_eq!(run(&p, t.arch, call, (1u64 << 32) | AF_UNIX as u64), ALLOW);
            assert_eq!(run(&p, t.arch, call, (1u64 << 32) | AF_VSOCK as u64), EPERM);
        }
    }
}

#[test]
fn sc04_ordinary_calls_are_untouched() {
    let p = seccomp::program();
    // read, write, openat, mmap, execve, clone, exit_group, futex, epoll_wait, eventfd2, pipe2
    let aarch64 = [63, 64, 56, 222, 221, 220, 94, 98, 22, 19, 59];
    let x86 = [0, 1, 257, 9, 59, 56, 231, 202, 232, 290, 293];
    for n in aarch64 {
        assert_eq!(run(&p, AARCH64.arch, n, 0), ALLOW, "aarch64 {n}");
    }
    for n in x86 {
        assert_eq!(run(&p, X86_64.arch, n, 0), ALLOW, "x86-64 {n}");
    }
}

#[test]
fn sc05_another_architecture_or_the_x32_abi_kills_the_process() {
    let p = seccomp::program();
    let i386 = 0x4000_0003;
    let arm = 0x4000_0028;
    for arch in [i386, arm, 0, u32::MAX] {
        for n in [0, 1, 41, 102, 198, 358] {
            assert_eq!(run(&p, arch, n, 0), KILL, "arch {arch:#x} call {n}");
        }
    }
    // x32 calls reuse the x86-64 architecture value with bit 30 set: x32 socket is 0x40000000 + 41
    for n in [
        0x4000_0000,
        0x4000_0000 + 41,
        0x4000_0000 + 101,
        0x4000_0000 + 425,
        u32::MAX,
    ] {
        assert_eq!(run(&p, X86_64.arch, n, 0), KILL, "x32 call {n:#x}");
    }
    // the same numbers on aarch64 have no such ABI: they are plain unknown calls, allowed through to ENOSYS
    assert_eq!(run(&p, AARCH64.arch, 0x4000_0000 + 41, 0), ALLOW);
}

#[test]
fn sc06_the_denied_numbers_of_the_build_architecture_match_the_libc_crate() {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let (t, libc_numbers) = (
        &X86_64,
        [
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ],
    );
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let (t, libc_numbers) = (
        &AARCH64,
        [
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ],
    );
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        let mut ours = vec![t.socket, t.socketpair];
        ours.extend(t.denied);
        let want: Vec<u32> = libc_numbers.iter().map(|n| *n as u32).collect();
        assert_eq!(ours, want);
    }
}

#[test]
fn sc07_the_tables_hold_the_system_call_numbers_of_the_linux_headers_for_both_architectures() {
    // typed in from the libc crate's gnu/b64/x86_64/not_x32.rs and gnu/b64/aarch64/mod.rs, not computed from the tables
    assert_eq!(
        (X86_64.arch, X86_64.socket, X86_64.socketpair, X86_64.denied),
        (0xC000_003E, 41, 53, [101, 310, 311, 425, 426, 427])
    );
    assert_eq!(
        (
            AARCH64.arch,
            AARCH64.socket,
            AARCH64.socketpair,
            AARCH64.denied
        ),
        (0xC000_00B7, 198, 199, [117, 270, 271, 425, 426, 427])
    );
    assert_eq!([X86_64.x32_possible, AARCH64.x32_possible], [true, false]);
}
