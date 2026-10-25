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
    sys_open, sys_read, sys_close, sys_kill, sys_wait,
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

    // Прочитать cfg
    let mut cfg = [0u8; 1024];
    let cfg_len = read_cfg("/system/conf/init.cfg", &mut cfg);

    if cfg_len == 0 {
        uart::puts(b"init: cfg missing, fallback\n");
        let _ = sys_spawn("/system/bin/uart", 2);
    } else {
        let cfg_str = match core::str::from_utf8(&cfg[..cfg_len]) {
            Ok(s) => s,
            Err(_) => { uart::puts(b"init: cfg not utf8\n"); "" }
        };
        for line in cfg_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut parts = line.split_whitespace();
            let path = match parts.next() { Some(p) => p, None => continue };
            let prio: u8 = match parts.next().and_then(|s| s.parse::<u8>().ok()) {
                Some(p) => p, None => 2,
            };
            let r = sys_spawn(path, prio as u64);
            if r < 0 {
                uart::puts(b"init: spawn FAILED for ");
                uart::puts(path.as_bytes());
                uart::puts(b"\n");
            }
        }
    }

    // Дать uart подняться до sys_recv
    sys_sleep(3);

    // IPC #1 → uart
    let text = b"hello from init\n";
    let msg = Message {
        tag: 1,
        data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
    };
    let _ = sys_send(1, &msg);
    uart::puts(b"init: sent IPC #1\n");

    sys_sleep(2);

    // === A.4 TEST: kill + wait + respawn ===
    uart::puts(b"init: killing pid=1\n");
    let r_kill = sys_kill(1);
    if r_kill == 0 {
        uart::puts(b"init: kill ok\n");
    } else {
        uart::puts(b"init: kill FAILED\n");
    }

    let r_wait = sys_wait(1);
    if r_wait == -1 {
        uart::puts(b"init: wait returned -1 (killed)\n");
    } else if r_wait >= 0 {
        uart::puts(b"init: wait returned code >= 0\n");
    } else {
        uart::puts(b"init: wait returned unknown\n");
    }

    // Respawn
    uart::puts(b"init: respawning uart\n");
    let new_pid = sys_spawn("/system/bin/uart", 2);
    if new_pid > 0 {
        uart::puts(b"init: spawned new uart\n");
    } else {
        uart::puts(b"init: respawn FAILED\n");
    }

    sys_sleep(2);

    // IPC #2 → новый uart
    let msg2 = Message {
        tag: 1,
        data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
    };
    let _ = sys_send(new_pid as u64, &msg2);
    uart::puts(b"init: sent IPC #2 to new pid\n");

    loop { sys_sleep(1); }
}