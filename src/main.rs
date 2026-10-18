#![no_std]
#![no_main]

use core::arch::{asm, global_asm};

global_asm!(
    ".section .text._start",
    ".global _start",
    "_start:",
    "  bl user_main",
    "  mov x0, #0",
    "  mov x8, #2",
    "  svc #0",
    "1: b 1b",
);

fn sys_write(fd: u64, buf: *const u8, len: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "mov x8, #1",
            "svc #0",
            inlateout("x0") fd => ret,
            in("x1") buf,
            in("x2") len,
            out("x8") _,
        );
    }
    ret
}

fn sys_exit(code: u64) -> ! {
    unsafe {
        asm!(
            "mov x8, #2",
            "svc #0",
            in("x0") code,
            options(noreturn),
        );
    }
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let msg = b"Hello from Rust user!\n";
    sys_write(1, msg.as_ptr(), msg.len() as u64);
    sys_exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_exit(1);
}