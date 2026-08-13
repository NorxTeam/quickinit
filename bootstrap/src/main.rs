#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_WAIT: u64 = 61;
const SYS_EXIT: u64 = 60;
const SYS_GETPID: u64 = 39;
const SYS_WRITE: u64 = 1;
const SYS_SPAWN: u64 = 400;
const ECHILD: u64 = 10;

const BOOTING: &[u8] =
    b"\x1b[90m[\x1b[0m\x1b[92m  OK  \x1b[0m\x1b[90m]\x1b[0m quickinit: bootstrap entered\n";
const CHILD_PATH: &[u8] = b"/bin/rust-smoke";
const READY: &[u8] = b"\x1b[90m[\x1b[0m\x1b[92m  OK  \x1b[0m\x1b[90m]\x1b[0m quickinit: bootstrap ready; child reaped\n";
const FAIL: &[u8] =
    b"\x1b[90m[\x1b[0m\x1b[91m FAIL \x1b[0m\x1b[90m]\x1b[0m quickinit: bootstrap contract failed\n";

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    exit(101)
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    if write(1, BOOTING) != BOOTING.len() {
        write(2, FAIL);
        exit(1);
    }

    let pid = getpid();
    let wait_result = syscall1(SYS_WAIT, 0);
    if is_error(pid) || pid != 1 || !is_error(wait_result) || wait_result != error(ECHILD) {
        write(2, FAIL);
        exit(2);
    }

    let child = syscall2(SYS_SPAWN, CHILD_PATH.as_ptr() as u64, CHILD_PATH.len() as u64);
    if is_error(child) || child == 0 {
        write(2, FAIL);
        exit(4);
    }
    if syscall1(SYS_WAIT, child) != child {
        write(2, FAIL);
        exit(5);
    }

    if write(1, READY) != READY.len() {
        write(2, FAIL);
        exit(3);
    }
    exit(0)
}

fn write(fd: u64, bytes: &[u8]) -> usize {
    let result = syscall3(SYS_WRITE, fd, bytes.as_ptr() as u64, bytes.len() as u64);
    if is_error(result) {
        0
    } else {
        result as usize
    }
}

fn getpid() -> u64 {
    syscall0(SYS_GETPID)
}

fn exit(status: u64) -> ! {
    let _ = syscall1(SYS_EXIT, status);
    loop {
        core::hint::spin_loop();
    }
}

const fn error(errno: u64) -> u64 {
    0u64.wrapping_sub(errno)
}

const fn is_error(value: u64) -> bool {
    value >= u64::MAX - 4095
}

#[cfg(target_arch = "x86_64")]
fn syscall0(number: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "x86_64")]
fn syscall1(number: u64, arg0: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") arg0,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "x86_64")]
fn syscall3(number: u64, arg0: u64, arg1: u64, arg2: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "x86_64")]
fn syscall2(number: u64, arg0: u64, arg1: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") arg0,
            in("rsi") arg1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn syscall0(number: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "svc 0",
            inlateout("x0") 0u64 => result,
            in("x8") number,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn syscall1(number: u64, arg0: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "svc 0",
            inlateout("x0") arg0 => result,
            in("x8") number,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn syscall3(number: u64, arg0: u64, arg1: u64, arg2: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "svc 0",
            inlateout("x0") arg0 => result,
            inlateout("x1") arg1 => _,
            inlateout("x2") arg2 => _,
            in("x8") number,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn syscall2(number: u64, arg0: u64, arg1: u64) -> u64 {
    let result;
    unsafe {
        asm!(
            "svc 0",
            inlateout("x0") arg0 => result,
            in("x1") arg1,
            in("x8") number,
            options(nostack)
        );
    }
    result
}
