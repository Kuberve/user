#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_sleep,
    sys_open, sys_write_file, sys_close,
    http_get,
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

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"httpc: starting\n");
    sys_sleep(2);

    // MVP: hardcoded target. Later: read from /system/conf/repo.cfg.
    let host = [10, 0, 2, 2];   // QEMU host
    let port: u16 = 8080;
    let path = "/index.txt";
    let out_path = "/tmp/index.txt";

    uart::puts(b"httpc: GET ");
    uart::puts(path.as_bytes());
    uart::puts(b"\n");

    match http_get(host, port, path) {
        Ok(body) => {
            uart::puts(b"httpc: body ");
            print_u64(body.len() as u64);
            uart::puts(b" bytes\n");

            // First line of body (debug)
            uart::puts(b"httpc: first line: ");
            for &c in body {
                if c == b'\n' { break; }
                if c == b'\r' { continue; }
                uart::putc(c);
            }
            uart::puts(b"\n");

            // Save
            let ofd = sys_open(out_path, flags::O_CREAT);
            if ofd < 0 {
                uart::puts(b"httpc: open out FAILED\n");
                sys_exit(1);
            }
            let w = sys_write_file(ofd, body.as_ptr(), body.len() as u64);
            sys_close(ofd);
            if w < 0 {
                uart::puts(b"httpc: write FAILED\n");
                sys_exit(1);
            }
            uart::puts(b"httpc: saved ");
            print_u64(w as u64);
            uart::puts(b" bytes to ");
            uart::puts(out_path.as_bytes());
            uart::puts(b"\n");
            sys_exit(0);
        }
        Err(e) => {
            uart::puts(b"httpc: FAILED: ");
            uart::puts(e.as_str().as_bytes());
            uart::puts(b"\n");
            sys_exit(1);
        }
    }
}