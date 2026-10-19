#![no_std]
#![no_main]

use core::arch::{asm, global_asm};

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

// ABI: должно совпадать с kernel/src/arch/aarch64/ipc.rs
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Message {
    pub tag: u64,
    pub data: [u64; 7],
}

// --- Syscalls ---

fn sys_write(fd: u64, buf: *const u8, len: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "mov x8, #1", "svc #0",
            inlateout("x0") fd => ret,
            in("x1") buf, in("x2") len,
            out("x8") _,
        );
    }
    ret
}

fn sys_exit(code: u64) -> ! {
    unsafe {
        asm!("mov x8, #2", "svc #0", in("x0") code, options(noreturn));
    }
}

fn sys_getpid() -> u64 {
    let ret: u64;
    unsafe {
        asm!("mov x8, #4", "svc #0", lateout("x0") ret, out("x8") _);
    }
    ret
}

fn sys_send(target: u64, msg: *const Message) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "mov x8, #5", "svc #0",
            inlateout("x0") target => ret,
            in("x1") msg,
            out("x8") _,
        );
    }
    ret
}

fn sys_recv(msg: *mut Message) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "mov x8, #6", "svc #0",
            inlateout("x0") msg as u64 => ret,
            out("x8") _,
        );
    }
    ret
}

// --- Helpers ---

fn print(s: &str) {
    sys_write(1, s.as_ptr(), s.len() as u64);
}

fn print_num(mut n: u64) {
    if n == 0 {
        sys_write(1, b"0".as_ptr(), 1);
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        sys_write(1, &buf[i], 1);
    }
}

// --- main ---

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();

    if pid == 2 {
        // Sender
        print("sender: pid=2, sending to 3\n");
        let msg = Message {
            tag: 42,
            data: [0xDEAD, 0xBEEF, 0, 0, 0, 0, 0],
        };
        let r = sys_send(3, &msg);
        if r == 0 {
            print("sender: OK\n");
        } else {
            print("sender: FAIL\n");
        }
        sys_exit(0);
    } else if pid == 3 {
        // Receiver
        print("receiver: pid=3, waiting\n");
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        print("receiver: got tag=");
        print_num(msg.tag);
        print(" data0=");
        print_num(msg.data[0]);
        print(" data1=");
        print_num(msg.data[1]);
        print("\n");
        sys_exit(0);
    } else {
        print("unknown pid\n");
        sys_exit(1);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_exit(1);
}