#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::{asm, global_asm};
use core::alloc::Layout;

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
    for _ in 0..2 {
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
            println!("[pid=3] TCP client: connect to 10.0.2.2:8080");
            let fd = kuberve_libc::sys_socket(2, 1);
            if fd < 0 { println!("[pid=3] socket failed"); sys_exit(1); }

            let dst_ip: u32 = 0x0A000202;   // 10.0.2.2
            let r = kuberve_libc::sys_connect(fd, dst_ip, 8080);
            if r < 0 { println!("[pid=3] connect failed"); sys_exit(1); }
            println!("[pid=3] CONNECTED to 10.0.2.2:8080");

            let msg = b"Hello from Kuberve!\n";
            let n = kuberve_libc::sys_tcp_send(fd, msg.as_ptr(), msg.len() as u64);
            println!("[pid=3] sent {} bytes", n);

            // Ждём reply
            let mut rbuf = [0u8; 256];
            for _attempt in 0..15 {
                let n = kuberve_libc::sys_tcp_recv(fd, rbuf.as_mut_ptr(), 256);
                if n > 0 {
                    println!("[pid=3] TCP reply: {} bytes", n);
                    let s = core::str::from_utf8(&rbuf[..n as usize]).unwrap_or("<bin>");
                    print!("[pid=3] content: '{}'", s);
                    println!();
                    break;
                }
                kuberve_libc::sys_sleep(1);   // 1 сек
            }
            kuberve_libc::sys_close(fd);
            sys_exit(0);
        }
        4 => client_hello(2, b"client2 says hi via IPC\n"),
        _ => sys_exit(1),
    }
}