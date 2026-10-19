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

fn sys_irq_ack(intid: u64) {
    unsafe {
        asm!(
            "mov x8, #7", "svc #0",
            inlateout("x0") intid => _,
            out("x8") _,
        );
    }
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

// --- UART MMIO ---
const UART_BASE: usize = 0x09000000;
const UART_DR: *mut u32 = UART_BASE as *mut u32;
const UART_FR: *const u32 = (UART_BASE + 0x18) as *const u32;
const UART_IMSC: *mut u32 = (UART_BASE + 0x38) as *mut u32;   // interrupt mask set clear
const UART_ICR:  *mut u32 = (UART_BASE + 0x44) as *mut u32;   // interrupt clear
const UART_CR:   *mut u32 = (UART_BASE + 0x30) as *mut u32;   // control
const UART_FR_RXFE: u32 = 1 << 4;                             // RX FIFO empty

fn uart_rx_init() {
    unsafe {
        // CR: UARTEN | TXE | RXE
        core::ptr::write_volatile(UART_CR, (1 << 0) | (1 << 8) | (1 << 9));
        // ICR: clear all pending
        core::ptr::write_volatile(UART_ICR, 0x7FF);
        // IMSC: RXIM bit 4 (receive interrupt mask)
        core::ptr::write_volatile(UART_IMSC, 1 << 4);
    }
}

fn uart_try_read() -> Option<u8> {
    unsafe {
        if (*UART_FR & UART_FR_RXFE) != 0 {
            None
        } else {
            Some((core::ptr::read_volatile(UART_DR) & 0xFF) as u8)
        }
    }
}

fn uart_putc(c: u8) {
    unsafe {
        while (*UART_FR & (1 << 5)) != 0 {}
        core::ptr::write_volatile(UART_DR, c as u32);
    }
}

fn uart_puts(s: &[u8]) {
    for &b in s {
        if b == b'\n' { uart_putc(b'\r'); }
        uart_putc(b);
    }
}

// --- UART server loop ---
fn uart_server_loop() -> ! {
    uart_puts(b"uart_server up, pid=2\n");
    uart_rx_init();
    // Первый enable IRQ 33 — с этого момента приходит RX IRQ
    sys_irq_ack(33);

    loop {
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        match msg.tag {
            1 => {
                // WRITE
                let ptr = msg.data[0] as *const u8;
                let len = msg.data[1] as usize;
                for i in 0..len {
                    let c = unsafe { *ptr.add(i) };
                    if c == b'\n' { uart_putc(b'\r'); }
                    uart_putc(c);
                }
            }
            2 => {
                // IRQ: прочитать все доступные байты из UART, echo
                while let Some(c) = uart_try_read() {
                    if c == b'\r' { uart_putc(b'\n'); }
                    else if c == b'\n' { uart_putc(b'\n'); }
                    else { uart_putc(c); }
                }
                sys_irq_ack(33);
            }
            _ => {}
        }
    }
}

fn client_hello(uart_pid: u64, text: &'static [u8]) -> ! {
    let msg = Message {
        tag: 1,
        data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
    };
    sys_send(uart_pid, &msg);
    sys_exit(0);
}

// --- main ---
#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();
    match pid {
        2 => uart_server_loop(),
        3 => client_hello(2, b"client1 says hi via IPC\n"),
        4 => client_hello(2, b"client2 says hi via IPC\n"),
        _ => sys_exit(1),
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_write(1, b"panic!\n".as_ptr(), 7);
    sys_exit(1);
}