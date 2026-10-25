#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_open, sys_read, sys_close, sys_write_file,
    sys_mkdir,
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

// === Tar parse state ===

const MAX_ENTRIES: usize = 32;
const MAX_NAME: usize = 100;
const FILE_DATA_SIZE: usize = 131072;  // 128 KB

#[derive(Clone, Copy)]
struct TarEntry {
    name: [u8; MAX_NAME],
    name_len: u8,
    size: u32,
    typeflag: u8,
    data_offset: u32,  // смещение в FILE_DATA
}

const EMPTY_ENTRY: TarEntry = TarEntry {
    name: [0u8; MAX_NAME],
    name_len: 0,
    size: 0,
    typeflag: 0,
    data_offset: 0,
};

static mut ENTRIES: [TarEntry; MAX_ENTRIES] = [EMPTY_ENTRY; MAX_ENTRIES];
static mut ENTRY_COUNT: usize = 0;
static mut FILE_DATA: [u8; FILE_DATA_SIZE] = [0; FILE_DATA_SIZE];
static mut DATA_OFFSET: usize = 0;

// === Утилиты ===

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

/// Собрать путь "/apps/<pkg>/<rel>" в буфер. Возвращает длину.
fn build_path(dst: &mut [u8], pkg: &[u8], rel: &[u8]) -> usize {
    let mut n = 0;
    for &b in b"/apps/" { dst[n] = b; n += 1; }
    for &b in pkg { dst[n] = b; n += 1; }
    if !rel.is_empty() {
        dst[n] = b'/'; n += 1;
        for &b in rel { dst[n] = b; n += 1; }
    }
    n
}

/// Извлечь "name=..." из manifest content.
fn find_field(content: &[u8], key: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut i = 0;
    while i < content.len() {
        // Проверить начало строки — key=
        let mut k = 0;
        while k < key.len() && i + k < content.len() && content[i + k] == key[k] { k += 1; }
        if k == key.len() && i + k < content.len() && content[i + k] == b'=' {
            let val_start = i + k + 1;
            let mut val_end = val_start;
            while val_end < content.len() && content[val_end] != b'\n' && content[val_end] != b'\r' {
                val_end += 1;
            }
            let vlen = val_end - val_start;
            if vlen > out.len() { return None; }
            for j in 0..vlen { out[j] = content[val_start + j]; }
            return Some(vlen);
        }
        // Перейти к следующей строке
        while i < content.len() && content[i] != b'\n' { i += 1; }
        i += 1;
    }
    None
}

// === Main ===

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    unsafe {
        uart::puts(b"kv: starting\n");

        let src = "/tmp/test.kvp";
        let fd = sys_open(src, 0);
        if fd < 0 {
            uart::puts(b"kv: open ");
            uart::puts(src.as_bytes());
            uart::puts(b" FAILED\n");
            sys_exit(1);
        }

        let mut hdr = [0u8; 512];
        ENTRY_COUNT = 0;
        DATA_OFFSET = 0;

        // === Pass 1: parse tar, сохранить headers + data ===
        loop {
            let n = sys_read(fd, hdr.as_mut_ptr(), 512);
            if n != 512 { break; }
            if is_zero_block(&hdr) { break; }
            if ENTRY_COUNT >= MAX_ENTRIES {
                uart::puts(b"kv: too many entries\n");
                break;
            }

            let mut name_len = 100usize;
            let mut i = 0;
            while i < 100 { if hdr[i] == 0 { name_len = i; break; } i += 1; }
            if name_len > MAX_NAME { name_len = MAX_NAME; }

            let size = parse_octal(&hdr[124..136]) as u32;
            let typeflag = hdr[156];

            let entry = &mut ENTRIES[ENTRY_COUNT];
            entry.name_len = name_len as u8;
            for j in 0..name_len { entry.name[j] = hdr[j]; }
            entry.size = size;
            entry.typeflag = typeflag;

            // Прочитать данные файла в FILE_DATA
            if typeflag != b'5' && size > 0 {
                if DATA_OFFSET + size as usize > FILE_DATA_SIZE {
                    uart::puts(b"kv: FILE_DATA overflow\n");
                    break;
                }
                entry.data_offset = DATA_OFFSET as u32;
                let dst = &mut FILE_DATA[DATA_OFFSET..DATA_OFFSET + size as usize];
                let mut rem = size as usize;
                let mut off = 0;
                while rem > 0 {
                    let chunk = if rem > 512 { 512 } else { rem };
                    let m = sys_read(fd, dst.as_mut_ptr().add(off), chunk as u64);
                    if m <= 0 { break; }
                    off += m as usize;
                    rem -= m as usize;
                }
                DATA_OFFSET += size as usize;
                // Skip padding
                let pad = (512 - (size as usize % 512)) % 512;
                if pad > 0 {
                    let mut pad_buf = [0u8; 512];
                    let _ = sys_read(fd, pad_buf.as_mut_ptr(), pad as u64);
                }
            } else {
                entry.data_offset = 0;
                // Dir: skip padding (size=0 → нет padding)
            }

            ENTRY_COUNT += 1;
        }

        sys_close(fd);
        uart::puts(b"kv: parsed ");
        print_u64(ENTRY_COUNT as u64);
        uart::puts(b" entries\n");

        // === Найти manifest.txt и вытащить name ===
        let mut pkg_name = [0u8; 16];
        let mut pkg_name_len = 0usize;
        for i in 0..ENTRY_COUNT {
            let e = &ENTRIES[i];
            let ename = &e.name[..e.name_len as usize];
            if ename == b"manifest.txt" {
                let data = &FILE_DATA[e.data_offset as usize..e.data_offset as usize + e.size as usize];
                if let Some(n) = find_field(data, b"name", &mut pkg_name) {
                    pkg_name_len = n;
                    break;
                }
            }
        }
        if pkg_name_len == 0 {
            uart::puts(b"kv: no name in manifest\n");
            sys_exit(1);
        }
        uart::puts(b"kv: pkg name=");
        uart::puts(&pkg_name[..pkg_name_len]);
        uart::puts(b"\n");

        // === Создать /apps/<pkg> ===
        let mut path_buf = [0u8; 256];
        let n = build_path(&mut path_buf, &pkg_name[..pkg_name_len], b"");
        let base_path = core::str::from_utf8(&path_buf[..n]).unwrap_or("");
        if sys_mkdir(base_path) != 0 {
            uart::puts(b"kv: mkdir base FAILED\n");
            sys_exit(1);
        }
        uart::puts(b"kv: mkdir ");
        uart::puts(base_path.as_bytes());
        uart::puts(b" ok\n");

        // === Установить все file entries ===
        let mut installed = 0u32;
        for i in 0..ENTRY_COUNT {
            let e = &ENTRIES[i];
            if e.typeflag == b'5' { continue; }   // dir, обработаем через mkdir parent'а
            let ename = &e.name[..e.name_len as usize];
            if ename == b"manifest.txt" {
                // Уже есть в родительском — установим как часть
            }

            // Создать parent dirs (простая версия: только 1 уровень — "bin/hello" → mkdir "bin")
            // Для MVP: если есть '/' в имени, mkdir parent
            let mut slash_pos = None;
            for (j, &b) in ename.iter().enumerate() {
                if b == b'/' { slash_pos = Some(j); }
            }
            if let Some(sp) = slash_pos {
                let parent_rel = &ename[..sp];
                let pn = build_path(&mut path_buf, &pkg_name[..pkg_name_len], parent_rel);
                let ppath = core::str::from_utf8(&path_buf[..pn]).unwrap_or("");
                let _ = sys_mkdir(ppath);   // idempotent
            }

            // sys_open(dest, O_CREAT)
            let dn = build_path(&mut path_buf, &pkg_name[..pkg_name_len], ename);
            let dest = core::str::from_utf8(&path_buf[..dn]).unwrap_or("");
            let dfd = sys_open(dest, flags::O_CREAT);
            if dfd < 0 {
                uart::puts(b"kv: open ");
                uart::puts(dest.as_bytes());
                uart::puts(b" FAILED\n");
                continue;
            }

            // Записать данные
            if e.size > 0 {
                let data_ptr = unsafe { FILE_DATA.as_ptr().add(e.data_offset as usize) };
                let written = sys_write_file(dfd, data_ptr, e.size as u64);
                if written != e.size as i64 {
                    uart::puts(b"kv: write short\n");
                }
            }
            sys_close(dfd);

            installed += 1;
        }

        uart::puts(b"kv: installed ");
        print_u64(installed as u64);
        uart::puts(b" files\n");

        sys_exit(0);
    }
}