#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    Message,
    BumpAlloc,
    sys_exit, sys_sleep, sys_send, sys_spawn,
    uart,
};

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

#[global_allocator]
static GLOBAL: BumpAlloc = BumpAlloc;

#[alloc_error_handler]
fn alloc_error(_: Layout) -> ! {
    kuberve_libc::sys_write(1, b"alloc error\n".as_ptr(), 12);
    sys_exit(2);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    kuberve_libc::sys_write(1, b"panic!\n".as_ptr(), 7);
    sys_exit(1);
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"init: starting\n");

    // Спавним uart_server из /system/bin/uart
    let r = sys_spawn("/system/bin/uart", 2);
    if r < 0 {
        uart::puts(b"init: spawn uart FAILED\n");
    }

    // Дать child'у подняться до sys_recv
    sys_sleep(1);

    // IPC: hello to uart_server (pid=1)
    let text = b"hello from init\n";
    let msg = Message {
        tag: 1,
        data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
    };
    let _ = sys_send(1, &msg);

    loop { sys_sleep(1); }
}