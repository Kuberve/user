#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::{asm, global_asm};
use core::alloc::{GlobalAlloc, Layout};
use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

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

// --- ABI ---
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
        asm!("mov x8, #1", "svc #0",
            inlateout("x0") fd => ret,
            in("x1") buf, in("x2") len, out("x8") _);
    }
    ret
}

fn sys_exit(code: u64) -> ! {
    unsafe { asm!("mov x8, #2", "svc #0", in("x0") code, options(noreturn)); }
}

fn sys_getpid() -> u64 {
    let ret: u64;
    unsafe { asm!("mov x8, #4", "svc #0", lateout("x0") ret, out("x8") _); }
    ret
}

fn sys_send(target: u64, msg: *const Message) -> i64 {
    let ret: i64;
    unsafe {
        asm!("mov x8, #5", "svc #0",
            inlateout("x0") target => ret,
            in("x1") msg, out("x8") _);
    }
    ret
}

fn sys_recv(msg: *mut Message) -> i64 {
    let ret: i64;
    unsafe {
        asm!("mov x8, #6", "svc #0",
            inlateout("x0") msg as u64 => ret,
            out("x8") _);
    }
    ret
}

// --- Heap (bump allocator) ---
const HEAP_START: usize = 0x40800000;
const HEAP_END:   usize = 0x40A00000;

static HEAP_NEXT: AtomicUsize = AtomicUsize::new(0);

struct BumpAlloc;

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut next = HEAP_NEXT.load(Ordering::Relaxed);
        if next == 0 { next = HEAP_START; }
        let align = layout.align().max(8);
        let aligned = (next + align - 1) & !(align - 1);
        let end = match aligned.checked_add(layout.size()) {
            Some(e) => e,
            None => return ptr::null_mut(),
        };
        if end > HEAP_END {
            return ptr::null_mut();
        }
        HEAP_NEXT.store(end, Ordering::Relaxed);
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // no-op
    }
}

#[global_allocator]
static GLOBAL: BumpAlloc = BumpAlloc;

#[alloc_error_handler]
fn alloc_error(_: Layout) -> ! {
    sys_write(1, b"alloc error\n".as_ptr(), 12);
    sys_exit(2);
}

// --- print! / println! ---
pub struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut buf = s.as_bytes();
        while !buf.is_empty() {
            let n = sys_write(1, buf.as_ptr(), buf.len() as u64);
            if n <= 0 { return fmt::Result::Err(fmt::Error); }
            buf = &buf[n as usize..];
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($a:tt)*) => {
        let _ = core::fmt::write(&mut $crate::Stdout, format_args!($($a)*));
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n"); };
    ($($a:tt)*) => {{
        $crate::print!($($a)*);
        $crate::print!("\n");
    }};
}

// --- main ---
#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();

    // Тест libc
    println!("[pid={}] hello from libc", pid);

    let mut v: Vec<u32> = Vec::new();
    for i in 0..10 { v.push(i * 10); }
    println!("[pid={}] vec: len={} first={} last={}", pid, v.len(), v[0], v[v.len()-1]);

    let s: String = String::from("dynamic string");
    println!("[pid={}] string: {}", pid, s);

    let f = format!("format: {} + {} = {}", 2, 3, 5);
    println!("[pid={}] {}", pid, f);

    // Тест IPC
    if pid == 2 {
        let msg = Message { tag: 42, data: [0xDEAD, 0xBEEF, 0, 0, 0, 0, 0] };
        let r = sys_send(3, &msg);
        println!("[pid=2] sender: {}", if r == 0 { "OK" } else { "FAIL" });
        sys_exit(0);
    } else {
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        println!("[pid=3] receiver: tag={} d0={:#x} d1={:#x}", msg.tag, msg.data[0], msg.data[1]);
        sys_exit(0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_write(1, b"panic!\n".as_ptr(), 7);
    sys_exit(1);
}