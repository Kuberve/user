#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_sleep,
    sys_gui_create_window,
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

fn print_u64(mut n: u64) {
    if n == 0 { uart::putc(b'0'); return; }
    let mut buf = [0u8; 20];
    let mut j = 0;
    while n > 0 { buf[j] = b'0' + (n % 10) as u8; n /= 10; j += 1; }
    while j > 0 { j -= 1; uart::putc(buf[j]); }
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"hello_win: starting\n");

    // Ждём GPU init в kmain (200-300ms обычно, берём с запасом 3 сек)
    // Пробуем каждую секунду: если gpu::get_global_mut() == None
    // → sys_gui_create_window вернёт -1.
    let mut id: i64 = -1;
    for _ in 0..5 {
        id = sys_gui_create_window(300, 200, "Hello Win");
        if id >= 0 { break; }
        sys_sleep(1);
    }

    if id >= 0 {
        uart::puts(b"hello_win: created id=");
        print_u64(id as u64);
        uart::puts(b"\n");
    } else {
        uart::puts(b"hello_win: FAILED (gpu not ready?)\n");
    }

    loop { sys_sleep(1); }
}