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
    sys_open, sys_read, sys_close,
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

fn read_cfg(path: &str, buf: &mut [u8]) -> usize {
    let fd = sys_open(path, 0);
    if fd < 0 { return 0; }
    let n = sys_read(fd, buf.as_mut_ptr(), buf.len() as u64);
    sys_close(fd);
    if n <= 0 { 0 } else { n as usize }
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"init: starting\n");

    // Прочитать /system/conf/init.cfg
    let mut cfg = [0u8; 1024];
    let cfg_len = read_cfg("/system/conf/init.cfg", &mut cfg);

    if cfg_len == 0 {
        uart::puts(b"init: cfg missing, fallback to /system/bin/uart\n");
        let _ = sys_spawn("/system/bin/uart", 2);
    } else {
        // Parse: <path> <priority>, # комментарии
        let cfg_str = match core::str::from_utf8(&cfg[..cfg_len]) {
            Ok(s) => s,
            Err(_) => {
                uart::puts(b"init: cfg not utf8, fallback\n");
                let _ = sys_spawn("/system/bin/uart", 2);
                sys_sleep(1);
                let text = b"hello from init\n";
                let msg = Message {
                    tag: 1,
                    data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
                };
                let _ = sys_send(1, &msg);
                loop { sys_sleep(1); }
            }
        };

        for line in cfg_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }

            let mut parts = line.split_whitespace();
            let path = match parts.next() { Some(p) => p, None => continue };
            let prio: u8 = match parts.next().and_then(|s| s.parse::<u8>().ok()) {
                Some(p) => p,
                None => 2,
            };

            let r = sys_spawn(path, prio as u64);
            if r < 0 {
                uart::puts(b"init: spawn FAILED for ");
                uart::puts(path.as_bytes());
                uart::puts(b"\n");
            }
        }
    }

    // Дать сервисам подняться до sys_recv
    sys_sleep(1);

    // IPC hello — для проверки end-to-end
    let text = b"hello from init\n";
    let msg = Message {
        tag: 1,
        data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
    };
    let _ = sys_send(1, &msg);

    loop { sys_sleep(1); }
}