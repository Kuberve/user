#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::{asm, global_asm};
use core::alloc::Layout;
use core::fmt;

use kuberve_libc::{
    Message,
    BumpAlloc,
    sys_exit, sys_getpid, sys_send, sys_recv, sys_irq_ack,
    uart,
};
use kuberve_libc::{print, println};

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
    uart::puts(b"uart_server up, pid=2\n");
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

fn client_hello(uart_pid: u64, text: &'static [u8]) -> ! {
    for _ in 0..20 {
        let msg = Message {
            tag: 1,
            data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
        };
        sys_send(uart_pid, &msg);
    }
    sys_exit(0);
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();
    match pid {
        2 => uart_server_loop(),
                3 => {
            // 1. Прочитать оригинал
            println!("[pid=3] reading HELLO.TXT");
            let fd = kuberve_libc::sys_open("HELLO.TXT", 0);
            if fd < 0 { println!("[pid=3] open failed"); sys_exit(1); }
            let mut buf = [0u8; 64];
            let n = kuberve_libc::sys_read(fd, buf.as_mut_ptr(), 64);
            kuberve_libc::sys_close(fd);
            let s = core::str::from_utf8(&buf[..n as usize]).unwrap_or("?");
            println!("[pid=3] original: '{}'", s.trim_end());

            // 2. Записать "Hi FAT32!"
            println!("[pid=3] writing 'Hi FAT32!'");
            let fd = kuberve_libc::sys_open("HELLO.TXT", 0);
            if fd < 0 { println!("[pid=3] open for write failed"); sys_exit(1); }
            let data = b"Hello FAT32, v2!\n";
            let w = kuberve_libc::sys_write_file(fd, data.as_ptr(), data.len() as u64);
            kuberve_libc::sys_close(fd);
            if w < 0 {
                println!("[pid=3] write failed: {}", w);
                sys_exit(1);
            }
            println!("[pid=3] wrote {} bytes", w);

            // 3. Прочитать обратно
            let fd = kuberve_libc::sys_open("HELLO.TXT", 0);
            if fd < 0 { sys_exit(1); }
            let n = kuberve_libc::sys_read(fd, buf.as_mut_ptr(), 64);
            kuberve_libc::sys_close(fd);
            let s = core::str::from_utf8(&buf[..n as usize]).unwrap_or("?");
            println!("[pid=3] after write: '{}'", s.trim_end());

            sys_exit(0);
        }
        4 => client_hello(2, b"client2 says hi via IPC\n"),
        _ => sys_exit(1),
    }
}