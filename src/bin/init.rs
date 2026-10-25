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
    sys_stat, sys_mkdir, sys_unlink, sys_rmdir,
    sys_readdir, sys_write_file,
    uart,
    flags,
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

    // === F.0+F.1 test: stat на существующих файлах ===
    {
        let mut st = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
        let r = sys_stat("/system/bin/uart", &mut st);
        if r == 0 {
            uart::puts(b"init: stat uart ok, size=");
            let mut buf = [0u8; 20];
            let mut n = st.size;
            if n == 0 { buf[0] = b'0'; uart::puts(&buf[..1]); }
            else {
                let mut i = 0;
                while n > 0 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
                while i > 0 { i -= 1; uart::putc(buf[i]); }
            }
            uart::puts(b" is_dir=");
            uart::putc(b'0' + (st.is_dir as u8));
            uart::puts(b"\n");
        } else {
            uart::puts(b"init: stat uart FAILED\n");
        }

        let mut st2 = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
        let r2 = sys_stat("/system", &mut st2);
        if r2 == 0 {
            uart::puts(b"init: stat /system ok, is_dir=");
            uart::putc(b'0' + (st2.is_dir as u8));
            uart::puts(b"\n");
        } else {
            uart::puts(b"init: stat /system FAILED\n");
        }
    }

    // === F.2 test: mkdir (idempotent) ===
    {
        let r = sys_mkdir("/users/0/test");
        if r == 0 {
            uart::puts(b"init: mkdir ok (created or exists)\n");
        } else {
            uart::puts(b"init: mkdir FAILED\n");
        }

        let mut st = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
        let r2 = sys_stat("/users/0/test", &mut st);
        if r2 == 0 && st.is_dir == 1 {
            uart::puts(b"init: stat test dir ok, is_dir=1\n");
        } else {
            uart::puts(b"init: stat test dir FAILED\n");
        }
    }

    // === F.3 test: create file in nested dir ===
    {
        let fd = sys_open("/users/0/test/hello.txt", flags::O_CREAT);
        if fd >= 0 {
            uart::puts(b"init: create file ok\n");
            sys_close(fd);

            let mut st = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
            let r = sys_stat("/users/0/test/hello.txt", &mut st);
            if r == 0 && st.is_dir == 0 && st.size == 0 {
                uart::puts(b"init: stat created file ok, size=0\n");
            } else {
                uart::puts(b"init: stat created file FAILED\n");
            }
        } else {
            uart::puts(b"init: create file FAILED\n");
        }
    }

    // === F.4 test: unlink + rmdir ===
    {
        let r = sys_unlink("/users/0/test/hello.txt");
        if r == 0 {
            uart::puts(b"init: unlink ok\n");
        } else {
            uart::puts(b"init: unlink FAILED\n");
        }

        let mut st = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
        if sys_stat("/users/0/test/hello.txt", &mut st) != 0 {
            uart::puts(b"init: stat after unlink missing (correct)\n");
        } else {
            uart::puts(b"init: stat after unlink still there (BUG)\n");
        }

        let r2 = sys_rmdir("/users/0/test");
        if r2 == 0 {
            uart::puts(b"init: rmdir ok\n");
        } else {
            uart::puts(b"init: rmdir FAILED\n");
        }

        let mut st2 = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
        if sys_stat("/users/0/test", &mut st2) != 0 {
            uart::puts(b"init: stat after rmdir missing (correct)\n");
        } else {
            uart::puts(b"init: stat after rmdir still there (BUG)\n");
        }
    }

    // === F.5 test: readdir ===
    {
        let empty_entry = kuberve_libc::DirEntry {
            name: [0u8; 20],
            is_dir: 0, size: 0, cluster: 0,
        };

        // /system/bin → INIT, UART
        let mut entries = [empty_entry; 16];
        let n = sys_readdir("/system/bin", entries.as_mut_ptr(), 16);
        if n > 0 {
            uart::puts(b"init: readdir /system/bin count=");
            uart::putc(b'0' + (n as u8));
            uart::puts(b"\n");
            for i in 0..(n as usize) {
                let e = &entries[i];
                let mut end = 0;
                while end < 20 && e.name[end] != 0 { end += 1; }
                uart::puts(b"  ");
                uart::puts(&e.name[..end]);
                if e.is_dir == 1 { uart::puts(b"/"); }
                uart::puts(b"\n");
            }
        } else {
            uart::puts(b"init: readdir /system/bin FAILED\n");
        }

        // /system → BIN, CONF, BOOT
        let mut entries2 = [empty_entry; 16];
        let n2 = sys_readdir("/system", entries2.as_mut_ptr(), 16);
        if n2 > 0 {
            uart::puts(b"init: readdir /system count=");
            uart::putc(b'0' + (n2 as u8));
            uart::puts(b"\n");
            for i in 0..(n2 as usize) {
                let e = &entries2[i];
                let mut end = 0;
                while end < 20 && e.name[end] != 0 { end += 1; }
                uart::puts(b"  ");
                uart::puts(&e.name[..end]);
                if e.is_dir == 1 { uart::puts(b"/"); }
                uart::puts(b"\n");
            }
        } else {
            uart::puts(b"init: readdir /system FAILED\n");
        }
    }

    // === F.3.1 test: write в nested файл ===
    {
        let _ = sys_mkdir("/users/0/test");   // ensure существует (idempotent)

        let fd = sys_open("/users/0/test/data.txt", flags::O_CREAT);
        if fd >= 0 {
            let data = b"hello nested write";
            let written = sys_write_file(fd, data.as_ptr(), data.len() as u64);
            sys_close(fd);

            if written == data.len() as i64 {
                uart::puts(b"init: write nested ok\n");
            } else {
                uart::puts(b"init: write nested returned wrong count\n");
            }

            let mut st = kuberve_libc::Stat { size: 0, is_dir: 0, cluster: 0 };
            if sys_stat("/users/0/test/data.txt", &mut st) == 0 && st.size == data.len() as u32 {
                uart::puts(b"init: stat after write size ok\n");
            } else {
                uart::puts(b"init: stat after write size WRONG\n");
            }

            // Прочитать обратно
            let rfd = sys_open("/users/0/test/data.txt", 0);
            if rfd >= 0 {
                let mut rbuf = [0u8; 32];
                let n = sys_read(rfd, rbuf.as_mut_ptr(), 32);
                sys_close(rfd);
                if n == data.len() as i64 && rbuf[..n as usize] == data[..] {
                    uart::puts(b"init: read back matches\n");
                } else {
                    uart::puts(b"init: read back MISMATCH\n");
                }
            }

            // Cleanup
            let _ = sys_unlink("/users/0/test/data.txt");
        } else {
            uart::puts(b"init: create data.txt FAILED\n");
        }
    }

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