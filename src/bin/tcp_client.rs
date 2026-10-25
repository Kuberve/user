#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_sleep,
    sys_socket, sys_connect, sys_tcp_send, sys_tcp_recv, sys_close,
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

fn putc_uint(mut n: u32) {
    if n == 0 { uart::putc(b'0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    while i > 0 { i -= 1; uart::putc(buf[i]); }
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"tcp_client: starting\n");

    // Ждём DHCP + ARP (kmain делает send_arp после DHCP).
    // sys_sleep(1) = 1 секунда.
    sys_sleep(2);

    // AF_INET (2), SOCK_STREAM (1)
    let fd = sys_socket(2, 1);
    if fd < 0 {
        uart::puts(b"tcp_client: socket FAILED\n");
        loop { sys_sleep(1); }
    }
    uart::puts(b"tcp_client: socket ok, fd=");
    putc_uint(fd as u32);
    uart::puts(b"\n");

    // 10.0.2.2 (QEMU gateway → host) : 8080
    let dst_ip: u32 = 0x0A000202;

    // Retry: ARP может не успеть, connect вернёт -1.
    // До 5 попыток с интервалом 1 сек.
    let mut conn_ok = false;
    for attempt in 0..5 {
        uart::puts(b"tcp_client: connect attempt ");
        putc_uint(attempt + 1);
        uart::puts(b"\n");
        let r = sys_connect(fd, dst_ip, 8080);
        if r >= 0 {
            conn_ok = true;
            uart::puts(b"tcp_client: CONNECTED\n");
            break;
        }
        sys_sleep(1);
    }

    if !conn_ok {
        uart::puts(b"tcp_client: connect FAILED (5 attempts)\n");
        sys_close(fd);
        loop { sys_sleep(1); }
    }

    // Отправить
    let msg = b"Hello from Kuberve!\n";
    let sent = sys_tcp_send(fd, msg.as_ptr(), msg.len() as u64);
    uart::puts(b"tcp_client: sent ");
    putc_uint(sent as u32);
    uart::puts(b" bytes\n");

    // Получить ответ (echo) — 10 попыток × 1 сек
    let mut rbuf = [0u8; 256];
    let mut got: i64 = 0;
    for _ in 0..10 {
        let n = sys_tcp_recv(fd, rbuf.as_mut_ptr(), 256);
        if n > 0 { got = n; break; }
        sys_sleep(1);
    }

    if got > 0 {
        uart::puts(b"tcp_client: reply ");
        putc_uint(got as u32);
        uart::puts(b" bytes: ");
        for i in 0..got as usize {
            uart::putc(rbuf[i]);
        }
        uart::puts(b"\n");
    } else {
        uart::puts(b"tcp_client: no reply (timeout)\n");
    }

    sys_close(fd);
    uart::puts(b"tcp_client: closed\n");

    loop { sys_sleep(1); }
}