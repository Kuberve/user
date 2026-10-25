#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_open, sys_read, sys_close,
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

fn parse_octal(buf: &[u8]) -> u64 {
    let mut n: u64 = 0;
    for &b in buf {
        if b == 0 || b == b' ' { break; }
        if b >= b'0' && b <= b'7' {
            n = n * 8 + (b - b'0') as u64;
        } else {
            break;
        }
    }
    n
}

fn is_zero_block(buf: &[u8; 512]) -> bool {
    let mut i = 0;
    while i < 512 {
        if buf[i] != 0 { return false; }
        i += 1;
    }
    true
}

fn print_u64(mut n: u64) {
    if n == 0 { uart::putc(b'0'); return; }
    let mut buf = [0u8; 20];
    let mut j = 0;
    while n > 0 { buf[j] = b'0' + (n % 10) as u8; n /= 10; j += 1; }
    while j > 0 { j -= 1; uart::putc(buf[j]); }
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"kv: starting\n");

    let path = "/tmp/test.kvp";
    let fd = sys_open(path, 0);
    if fd < 0 {
        uart::puts(b"kv: open ");
        uart::puts(path.as_bytes());
        uart::puts(b" FAILED\n");
        sys_exit(1);
    }

    let mut hdr = [0u8; 512];
    let mut count = 0u32;

    loop {
        let n = sys_read(fd, hdr.as_mut_ptr(), 512);
        if n != 512 { break; }
        if is_zero_block(&hdr) { break; }

        // name (offset 0..100, null-terminated)
        let mut name_end = 100usize;
        let mut i = 0;
        while i < 100 {
            if hdr[i] == 0 { name_end = i; break; }
            i += 1;
        }
        let name = &hdr[..name_end];

        let size = parse_octal(&hdr[124..136]);
        let typeflag = hdr[156];

        uart::puts(b"  ");
        uart::puts(name);
        uart::puts(b" (");
        print_u64(size);
        if typeflag == b'5' { uart::puts(b" dir)\n"); }
        else { uart::puts(b" file)\n"); }

        // skip data (rounded up to 512)
        let mut skip = (size + 511) & !511u64;
        while skip > 0 {
            let chunk = if skip > 512 { 512 } else { skip as usize };
            let m = sys_read(fd, hdr.as_mut_ptr(), chunk as u64);
            if m <= 0 { break; }
            skip -= m as u64;
        }

        count += 1;
    }

    sys_close(fd);
    uart::puts(b"kv: ");
    print_u64(count as u64);
    uart::puts(b" entries\n");

    sys_exit(0);
}