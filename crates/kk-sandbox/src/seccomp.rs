//! The system-call filter every worker runs under.
//!
//! Network isolation by namespace (`--unshare-all`) does not cover everything: a VM socket (`AF_VSOCK`) is not a network
//! namespace object, so code in the sandbox could still open one and reach the host from inside a virtual machine (this
//! was reproduced in the Linux VM before the filter existed). The filter therefore refuses, with `EPERM`:
//! - `socket` and `socketpair` for every address family except `AF_UNIX`;
//! - `io_uring_setup`, `io_uring_enter` and `io_uring_register`, which can open sockets without a `socket` call;
//! - `ptrace`, `process_vm_readv` and `process_vm_writev`.
//!
//! A call from any other architecture, or through the x86-64 32-bit-pointer ABI (system call numbers with bit 30 set),
//! kills the process, because the numbers below mean something else there.
//!
//! The program is classic BPF in the layout `bwrap --seccomp` reads: an array of `struct sock_filter`, little endian.

pub const AF_UNIX: u32 = 1;

const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;
const X32_BIT: u32 = 0x4000_0000;

const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO_EPERM: u32 = 0x0005_0001;
const RET_ALLOW: u32 = 0x7fff_0000;

const OFFSET_NR: u32 = 0;
const OFFSET_ARCH: u32 = 4;
const OFFSET_ARG0_LOW: u32 = 16;

const LD_ABS_W: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JGE_K: u16 = 0x35;
const RET_K: u16 = 0x06;

/// System call numbers of one architecture (from the Linux headers, cross-checked against the `libc` crate in the tests).
pub struct Table {
    pub arch: u32,
    pub socket: u32,
    pub socketpair: u32,
    pub denied: [u32; 6],
    pub x32_possible: bool,
}

pub const X86_64: Table = Table {
    arch: AUDIT_ARCH_X86_64,
    socket: 41,
    socketpair: 53,
    // ptrace, process_vm_readv, process_vm_writev, io_uring_setup, io_uring_enter, io_uring_register
    denied: [101, 310, 311, 425, 426, 427],
    x32_possible: true,
};

pub const AARCH64: Table = Table {
    arch: AUDIT_ARCH_AARCH64,
    socket: 198,
    socketpair: 199,
    denied: [117, 270, 271, 425, 426, 427],
    x32_possible: false,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insn {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

struct Pending {
    code: u16,
    k: u32,
    jt: Option<usize>,
    jf: Option<usize>,
}

fn ld(offset: u32) -> Pending {
    Pending {
        code: LD_ABS_W,
        k: offset,
        jt: None,
        jf: None,
    }
}

fn ret(value: u32) -> Pending {
    Pending {
        code: RET_K,
        k: value,
        jt: None,
        jf: None,
    }
}

fn jump(code: u16, k: u32, jt: Option<usize>, jf: Option<usize>) -> Pending {
    Pending { code, k, jt, jf }
}

/// The instructions for one architecture, placed at absolute index `base` of the whole program.
fn block(base: usize, t: &Table) -> Vec<Pending> {
    let denied = t.denied.len();
    let x32 = usize::from(t.x32_possible);
    // layout: ld nr, [x32 check], denied..., socket, socketpair, allow | SOCK: ld arg0, jeq AF_UNIX | ERRNO | ALLOW2 | [KILL]
    let allow_default = base + 1 + x32 + denied + 2;
    let sock = allow_default + 1;
    let errno = sock + 2;
    let allow_after_sock = errno + 1;
    let kill = allow_after_sock + 1;
    let mut b = vec![ld(OFFSET_NR)];
    if t.x32_possible {
        b.push(jump(JGE_K, X32_BIT, Some(kill), None));
    }
    for n in t.denied {
        b.push(jump(JEQ_K, n, Some(errno), None));
    }
    b.push(jump(JEQ_K, t.socket, Some(sock), None));
    b.push(jump(JEQ_K, t.socketpair, Some(sock), None));
    b.push(ret(RET_ALLOW));
    b.push(ld(OFFSET_ARG0_LOW));
    b.push(jump(JEQ_K, AF_UNIX, Some(allow_after_sock), None));
    b.push(ret(RET_ERRNO_EPERM));
    b.push(ret(RET_ALLOW));
    if t.x32_possible {
        b.push(ret(RET_KILL_PROCESS));
    }
    b
}

fn resolve(pending: Vec<Pending>) -> Vec<Insn> {
    pending
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let off = |to: Option<usize>| match to {
                None => 0u8,
                Some(t) => {
                    assert!(t > i, "a filter jump must go forward");
                    u8::try_from(t - i - 1).expect("a filter jump is at most 255 instructions")
                }
            };
            Insn {
                code: p.code,
                jt: off(p.jt),
                jf: off(p.jf),
                k: p.k,
            }
        })
        .collect()
}

pub fn program() -> Vec<Insn> {
    // prefix: ld arch; jeq aarch64 -> its block; jeq x86-64 -> its block; kill (any other architecture)
    const PREFIX: usize = 4;
    let a64_len = block(PREFIX, &AARCH64).len();
    let x64_start = PREFIX + a64_len;
    let mut p = vec![
        ld(OFFSET_ARCH),
        jump(JEQ_K, AARCH64.arch, Some(PREFIX), None),
        jump(JEQ_K, X86_64.arch, Some(x64_start), None),
        ret(RET_KILL_PROCESS),
    ];
    p.extend(block(PREFIX, &AARCH64));
    p.extend(block(x64_start, &X86_64));
    resolve(p)
}

/// The program as `bwrap --seccomp` reads it: 8 bytes per instruction, little endian.
pub fn bytes() -> Vec<u8> {
    let mut out = Vec::new();
    for i in program() {
        out.extend_from_slice(&i.code.to_le_bytes());
        out.push(i.jt);
        out.push(i.jf);
        out.extend_from_slice(&i.k.to_le_bytes());
    }
    out
}
