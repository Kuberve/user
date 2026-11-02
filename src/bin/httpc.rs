#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_sleep,
    sys_socket, sys_connect, sys_tcp_send, sys_tcp_recv,
    sys_open, sys_write_file, sys_close,
    uart, flags,
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
    uart::puts(b"httpc: alloc error\n");
    sys_exit(2);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    uart::puts(b"httpc: panic\n");
    sys_exit(1);
}

fn print_u64(mut n: u64) {
    if n == 0 { uart::putc(b'0'); return; }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while n > 0 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    while i > 0 { i -= 1; uart::putc(buf[i]); }
}

const AF_INET: u64 = 2;
const SOCK_STREAM: u64 = 1;

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"httpc: starting\n");

    // Wait for DHCP + ARP to settle
    sys_sleep(2);

    // MVP: hardcoded target (later: parse argv or /system/conf/repo.cfg)
    let dst_ip: u32 = 0x0A000202;   // 10.0.2.2 = QEMU host
    let dst_port: u16 = 8080;
    let path = "/index.txt";
    let out_path = "/tmp/index.txt";

    // 1. TCP socket
    let fd = sys_socket(AF_INET, SOCK_STREAM);
    if fd < 0 {
        uart::puts(b"httpc: socket FAILED\n");
        sys_exit(1);
    }
    uart::puts(b"httpc: socket ok, fd=");
    print_u64(fd as u64);
    uart::puts(b"\n");

    // 2. Connect with retry (ARP may not have settled)
    let mut connected = false;
    for attempt in 0..5 {
        uart::puts(b"httpc: connect attempt ");
        print_u64(attempt + 1);
        uart::puts(b"\n");
        if sys_connect(fd, dst_ip, dst_port as u64) >= 0 {
            connected = true;
            break;
        }
        sys_sleep(1);
    }
    if !connected {
        uart::puts(b"httpc: connect FAILED\n");
        sys_close(fd);
        sys_exit(1);
    }
    uart::puts(b"httpc: connected\n");

    // 3. Send HTTP/1.0 request
    let mut req = [0u8; 256];
    let mut req_len = 0;
    for &b in b"GET " { req[req_len] = b; req_len += 1; }
    for &b in path.as_bytes() { req[req_len] = b; req_len += 1; }
    for &b in b" HTTP/1.0\r\nHost: kuberve\r\n\r\n" { req[req_len] = b; req_len += 1; }

    let sent = sys_tcp_send(fd, req.as_ptr(), req_len as u64);
    if sent < 0 {
        uart::puts(b"httpc: send FAILED\n");
        sys_close(fd);
        sys_exit(1);
    }
    uart::puts(b"httpc: request sent, ");
    print_u64(sent as u64);
    uart::puts(b" bytes\n");

    // 4. Read response
    sys_sleep(1);
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    let mut idle = 0;
    while total < 4096 {
        let chunk = core::cmp::min(1024u64, (4096 - total) as u64);
        let n = sys_tcp_recv(fd, buf[total..].as_mut_ptr(), chunk);
        if n < 0 { break; }
        if n > 0 {
            total += n as usize;
            idle = 0;
        } else {
            idle += 1;
            if idle > 3 { break; }
            sys_sleep(1);
        }
    }
    sys_close(fd);

    uart::puts(b"httpc: received ");
    print_u64(total as u64);
    uart::puts(b" bytes\n");

    if total == 0 {
        uart::puts(b"httpc: no response\n");
        sys_exit(1);
    }

    // 5. Skip HTTP headers (\r\n\r\n)
    let mut body_start = 0;
    let mut i = 0;
    while i + 3 < total {
        if buf[i] == b'\r' && buf[i+1] == b'\n'
           && buf[i+2] == b'\r' && buf[i+3] == b'\n' {
            body_start = i + 4;
            break;
        }
        i += 1;
    }

    if body_start == 0 || body_start >= total {
        uart::puts(b"httpc: no body\n");
        sys_exit(1);
    }

    let body_len = total - body_start;
    uart::puts(b"httpc: body ");
    print_u64(body_len as u64);
    uart::puts(b" bytes\n");

    // Debug: first line of body
    uart::puts(b"httpc: first line: ");
    for j in body_start..core::cmp::min(body_start + 64, total) {
        let c = buf[j];
        if c == b'\n' { break; }
        if c == b'\r' { continue; }
        uart::putc(c);
    }
    uart::puts(b"\n");

    // 6. Save body to file
    let ofd = sys_open(out_path, flags::O_CREAT);
    if ofd < 0 {
        uart::puts(b"httpc: open out FAILED\n");
        sys_exit(1);
    }
    let written = sys_write_file(ofd, buf[body_start..].as_ptr(), body_len as u64);
    sys_close(ofd);
    if written < 0 {
        uart::puts(b"httpc: write FAILED\n");
        sys_exit(1);
    }

    uart::puts(b"httpc: saved ");
    print_u64(written as u64);
    uart::puts(b" bytes to ");
    uart::puts(out_path.as_bytes());
    uart::puts(b"\n");

    sys_exit(0);
}