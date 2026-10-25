#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    Message,
    BumpAlloc,
    sys_exit, sys_recv, sys_irq_ack,
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
    uart::puts(b"uart_server up\n");
    uart::rx_init();
    sys_irq_ack(33);

    loop {
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        match msg.tag {
            1 => {
                let bytes = kuberve_libc::write_payload(&msg);
                for &c in bytes {
                    if c == b'\n' { uart::putc(b'\r'); }
                    uart::putc(c);
                }
            }
            2 => {
                while let Some(c) = uart::try_read() {
                    if c == b'\r' || c == b'\n' { uart::putc(b'\n'); }
                    else { uart::putc(c); }
                }
                sys_irq_ack(33);
            }
            _ => {}
        }
    }
}