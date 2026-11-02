#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![allow(static_mut_refs)]
#![allow(dead_code)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    Message,
    BumpAlloc,
    sys_exit, sys_getpid, sys_recv, sys_irq_ack,
    sys_spawn, sys_sleep, sys_send,
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
    kuberve_libc::sys_exit(2);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    kuberve_libc::sys_write(1, b"panic!\n".as_ptr(), 7);
    kuberve_libc::sys_exit(1);
}

// --- UART server loop (userspace driver) ---
fn uart_server_loop() -> ! {
    uart::puts(b"uart_server up, pid=1\n");
    uart::rx_init();
    sys_irq_ack(33);

    loop {
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        match msg.tag {
            1 => {
                // WRITE: kernel уже упаковал байты в data[1..], data[0] = len
                let bytes = kuberve_libc::write_payload(&msg);
                for &c in bytes {
                    if c == b'\n' { uart::putc(b'\r'); }
                    uart::putc(c);
                }
            }
            2 => {
                // IRQ
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

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();
    match pid {
        0 => {
            // init: спавнит uart_server (child, pid=1) и уходит в сон
            let _ = sys_spawn("USER.ELF", 2);

            // Дать child'у подняться до sys_recv (он стартует с MMIO puts,
            // потом уходит в sys_recv loop)
            sys_sleep(10);   // 10 * 100 тиков = 10 сек? нет: sys_sleep(N) = N*TICK_HZ тиков.
                             // 10 * 100 = 1000 тиков = 10 сек. Слишком много.
                             // Лучше: sys_sleep(1) = 1 сек — хватит с головой.

            // Отправить сообщение в uart_server (pid=1)
            let text = b"hello from init\n";
            let msg = Message {
                tag: 1,  // WRITE
                data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
            };
            let _ = sys_send(1, &msg);

            loop { sys_sleep(1); }
        }
        1 => uart_server_loop(),
        _ => sys_exit(1),
    }
}