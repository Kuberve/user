#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::alloc::Layout;

use kuberve_libc::{
    BumpAlloc,
    sys_exit, sys_open, sys_read, sys_close, sys_write_file,
    sys_mkdir, sys_readdir, sys_unlink, sys_rmdir,
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

const MAX_ENTRIES: usize = 32;
const MAX_NAME: usize = 100;
const FILE_DATA_SIZE: usize = 131072;

#[derive(Clone, Copy)]
struct TarEntry {
    name: [u8; MAX_NAME],
    name_len: u8,
    size: u32,
    typeflag: u8,
    data_offset: u32,
}

const EMPTY_ENTRY: TarEntry = TarEntry {
    name: [0u8; MAX_NAME], name_len: 0, size: 0, typeflag: 0, data_offset: 0,
};

static mut ENTRIES: [TarEntry; MAX_ENTRIES] = [EMPTY_ENTRY; MAX_ENTRIES];
static mut ENTRY_COUNT: usize = 0;
static mut FILE_DATA: [u8; FILE_DATA_SIZE] = [0; FILE_DATA_SIZE];
static mut DATA_OFFSET: usize = 0;

fn parse_octal(buf: &[u8]) -> u64 {
    let mut n: u64 = 0;
    for &b in buf {
        if b == 0 || b == b' ' { break; }
        if b >= b'0' && b <= b'7' { n = n * 8 + (b - b'0') as u64; }
        else { break; }
    }
    n
}

fn is_zero_block(buf: &[u8; 512]) -> bool {
    for i in 0..512 { if buf[i] != 0 { return false; } }
    true
}

fn print_u64(mut n: u64) {
    if n == 0 { uart::putc(b'0'); return; }
    let mut buf = [0u8; 20];
    let mut j = 0;
    while n > 0 { buf[j] = b'0' + (n % 10) as u8; n /= 10; j += 1; }
    while j > 0 { j -= 1; uart::putc(buf[j]); }
}

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

/// path = "/apps/<pkg>/<rel>" → копирует в dst, возвращает длину.
fn join3(dst: &mut [u8], a: &[u8], b: &[u8]) -> usize {
    let mut n = 0;
    for &x in a { dst[n] = x; n += 1; }
    dst[n] = b'/'; n += 1;
    for &x in b { dst[n] = x; n += 1; }
    n
}

fn find_field(content: &[u8], key: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut i = 0;
    while i < content.len() {
        let mut k = 0;
        while k < key.len() && i + k < content.len() && content[i + k] == key[k] { k += 1; }
        if k == key.len() && i + k < content.len() && content[i + k] == b'=' {
            let val_start = i + k + 1;
            let mut val_end = val_start;
            while val_end < content.len() && content[val_end] != b'\n' && content[val_end] != b'\r' { val_end += 1; }
            let vlen = val_end - val_start;
            if vlen > out.len() { return None; }
            for j in 0..vlen { out[j] = content[val_start + j]; }
            return Some(vlen);
        }
        while i < content.len() && content[i] != b'\n' { i += 1; }
        i += 1;
    }
    None
}

fn read_manifest(pkg_name: &[u8], buf: &mut [u8]) -> usize {
    let mut path = [0u8; 256];
    let n = build_path(&mut path, pkg_name, b"manifest.txt");
    let path_str = match core::str::from_utf8(&path[..n]) { Ok(s) => s, Err(_) => return 0 };
    let fd = sys_open(path_str, 0);
    if fd < 0 { return 0; }
    let r = sys_read(fd, buf.as_mut_ptr(), buf.len() as u64);
    sys_close(fd);
    if r <= 0 { 0 } else { r as usize }
}

/// Рекурсивное удаление /apps/<pkg>/<subpath>. path_len — граница пути.
unsafe fn remove_recursive(path_buf: &mut [u8], path_len: usize) -> u32 {
    let mut removed = 0u32;
    let empty_de = kuberve_libc::DirEntry { name: [0u8; 20], is_dir: 0, size: 0, cluster: 0 };
    let mut entries = [empty_de; 16];

    let cur_path = match core::str::from_utf8(&path_buf[..path_len]) {
        Ok(s) => s, Err(_) => return 0,
    };
    let n = sys_readdir(cur_path, entries.as_mut_ptr(), 16);
    if n <= 0 { return 0; }

    for i in 0..(n as usize) {
        let e = &entries[i];
        let mut name_end = 0;
        while name_end < 20 && e.name[name_end] != 0 { name_end += 1; }
        let name = &e.name[..name_end];
        if name.is_empty() { continue; }

        let mut child_buf = [0u8; 256];
        for j in 0..path_len { child_buf[j] = path_buf[j]; }
        let _clen = join3(&mut child_buf[path_len..], b"", name) + path_len;
        // join3 с пустым a даст "/<name>", надо поправить: просто добавить "/name"
        // Упрощаем: копируем path_len + "/" + name
        let mut clen2 = path_len;
        child_buf[clen2] = b'/'; clen2 += 1;
        for &b in name { child_buf[clen2] = b; clen2 += 1; }

        if e.is_dir == 1 {
            removed += remove_recursive(&mut child_buf, clen2);
        } else {
            let child_str = core::str::from_utf8(&child_buf[..clen2]).unwrap_or("");
            if sys_unlink(child_str) == 0 { removed += 1; }
        }
    }

    let dir_str = core::str::from_utf8(&path_buf[..path_len]).unwrap_or("");
    let _ = sys_rmdir(dir_str);

    removed
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    unsafe {
        uart::puts(b"kv: starting\n");

        // ========== 1) Install ==========
        let src = "/tmp/test.kvp";
        let fd = sys_open(src, 0);
        if fd < 0 {
            uart::puts(b"kv: open src FAILED\n");
        } else {
            let mut hdr = [0u8; 512];
            ENTRY_COUNT = 0;
            DATA_OFFSET = 0;
            loop {
                let n = sys_read(fd, hdr.as_mut_ptr(), 512);
                if n != 512 { break; }
                if is_zero_block(&hdr) { break; }
                if ENTRY_COUNT >= MAX_ENTRIES { break; }

                let mut name_len = 100usize;
                for i in 0..100 { if hdr[i] == 0 { name_len = i; break; } }
                if name_len > MAX_NAME { name_len = MAX_NAME; }
                let size = parse_octal(&hdr[124..136]) as u32;
                let typeflag = hdr[156];

                let entry = &mut ENTRIES[ENTRY_COUNT];
                entry.name_len = name_len as u8;
                for j in 0..name_len { entry.name[j] = hdr[j]; }
                entry.size = size;
                entry.typeflag = typeflag;

                if typeflag != b'5' && size > 0 {
                    if DATA_OFFSET + size as usize > FILE_DATA_SIZE { break; }
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
                    let pad = (512 - (size as usize % 512)) % 512;
                    if pad > 0 {
                        let mut pb = [0u8; 512];
                        let _ = sys_read(fd, pb.as_mut_ptr(), pad as u64);
                    }
                } else {
                    entry.data_offset = 0;
                }
                ENTRY_COUNT += 1;
            }
            sys_close(fd);

            let mut pkg_name = [0u8; 16];
            let mut pkg_name_len = 0usize;
            for i in 0..ENTRY_COUNT {
                let e = &ENTRIES[i];
                if &e.name[..e.name_len as usize] == b"manifest.txt" {
                    let data = &FILE_DATA[e.data_offset as usize..e.data_offset as usize + e.size as usize];
                    if let Some(n) = find_field(data, b"name", &mut pkg_name) { pkg_name_len = n; break; }
                }
            }
            if pkg_name_len == 0 {
                uart::puts(b"kv: no name in manifest\n");
            } else {
                uart::puts(b"kv: pkg name=");
                uart::puts(&pkg_name[..pkg_name_len]);
                uart::puts(b"\n");

                let mut path_buf = [0u8; 256];
                let nb = build_path(&mut path_buf, &pkg_name[..pkg_name_len], b"");
                let base_path = core::str::from_utf8(&path_buf[..nb]).unwrap_or("");
                if sys_mkdir(base_path) != 0 {
                    uart::puts(b"kv: mkdir base FAILED\n");
                } else {
                    let mut installed = 0u32;
                    for i in 0..ENTRY_COUNT {
                        let e = &ENTRIES[i];
                        if e.typeflag == b'5' { continue; }
                        let ename = &e.name[..e.name_len as usize];

                        let mut slash_pos = None;
                        for (j, &b) in ename.iter().enumerate() { if b == b'/' { slash_pos = Some(j); } }
                        if let Some(sp) = slash_pos {
                            let pn = build_path(&mut path_buf, &pkg_name[..pkg_name_len], &ename[..sp]);
                            let ppath = core::str::from_utf8(&path_buf[..pn]).unwrap_or("");
                            let _ = sys_mkdir(ppath);
                        }

                        let dn = build_path(&mut path_buf, &pkg_name[..pkg_name_len], ename);
                        let dest = core::str::from_utf8(&path_buf[..dn]).unwrap_or("");
                        let dfd = sys_open(dest, flags::O_CREAT);
                        if dfd < 0 { continue; }
                        if e.size > 0 {
                            let dp = FILE_DATA.as_ptr().add(e.data_offset as usize);
                            let _ = sys_write_file(dfd, dp, e.size as u64);
                        }
                        sys_close(dfd);
                        installed += 1;
                    }
                    uart::puts(b"kv: installed ");
                    print_u64(installed as u64);
                    uart::puts(b" files\n");
                }
            }
        }

        // ========== 2) List ==========
        uart::puts(b"kv: --- list /apps ---\n");
        let empty_de = kuberve_libc::DirEntry { name: [0u8; 20], is_dir: 0, size: 0, cluster: 0 };
        let mut apps = [empty_de; 16];
        let n_apps = sys_readdir("/apps", apps.as_mut_ptr(), 16);
        if n_apps <= 0 {
            uart::puts(b"kv: /apps empty\n");
        } else {
            uart::puts(b"kv: /apps count=");
            print_u64(n_apps as u64);
            uart::puts(b"\n");
            for i in 0..(n_apps as usize) {
                let e = &apps[i];
                if e.is_dir != 1 { continue; }
                let mut name_end = 0;
                while name_end < 20 && e.name[name_end] != 0 { name_end += 1; }
                let dir_name = &e.name[..name_end];
                if dir_name.is_empty() { continue; }

                let mut mbuf = [0u8; 256];
                let mlen = read_manifest(dir_name, &mut mbuf);

                // Предпочитаем name из manifest, fallback — readdir (uppercase)
                let mut disp_name = [0u8; 16];
                let mut disp_len = 0usize;
                let mut ver = [0u8; 16];
                let mut desc = [0u8; 64];
                let mut vlen = 0usize;
                let mut dlen = 0usize;

                if mlen > 0 {
                    let mc = &mbuf[..mlen];
                    disp_len = find_field(mc, b"name", &mut disp_name).unwrap_or(0);
                    vlen = find_field(mc, b"version", &mut ver).unwrap_or(0);
                    dlen = find_field(mc, b"description", &mut desc).unwrap_or(0);
                }

                uart::puts(b"  ");
                if disp_len > 0 { uart::puts(&disp_name[..disp_len]); }
                else { uart::puts(dir_name); }
                uart::puts(b"  ");
                if vlen > 0 { uart::puts(&ver[..vlen]); } else { uart::puts(b"?"); }
                uart::puts(b"  ");
                if dlen > 0 { uart::puts(&desc[..dlen]); }
                uart::puts(b"\n");
            }
        }

        // ========== 3) Remove hello ==========
        uart::puts(b"kv: --- remove hello ---\n");
        let mut rm_path = [0u8; 256];
        let rm_nb = build_path(&mut rm_path, b"hello", b"");
        let removed = remove_recursive(&mut rm_path, rm_nb);
        uart::puts(b"kv: removed ");
        print_u64(removed as u64);
        uart::puts(b" entries\n");

        // ========== 4) List after remove ==========
        uart::puts(b"kv: --- list after remove ---\n");
        let mut apps2 = [empty_de; 16];
        let n2 = sys_readdir("/apps", apps2.as_mut_ptr(), 16);
        if n2 <= 0 {
            uart::puts(b"kv: /apps empty (correct)\n");
        } else {
            uart::puts(b"kv: /apps count=");
            print_u64(n2 as u64);
            uart::puts(b"\n");
        }

        // ========== 5) Update + Search ==========
        do_update_and_search();

        sys_exit(0);
    }
}

// ============================================================
// v0.15.5: kv update + kv search
// ============================================================

/// Read /system/conf/repo.cfg. Returns (host[4], port, index_path).
/// Fallback to 10.0.2.2:8080:/index.txt if cfg missing.
unsafe fn read_repo_cfg() -> ([u8; 4], u16, [u8; 64], usize) {
    let mut host = [10, 0, 2, 2];
    let mut port: u16 = 8080;
    let mut index = [0u8; 64];
    let mut index_len = 0usize;
    for (i, &b) in b"/index.txt".iter().enumerate() {
        index[i] = b;
        index_len += 1;
    }

    let fd = sys_open("/system/conf/repo.cfg", 0);
    if fd < 0 { return (host, port, index, index_len); }

    let mut buf = [0u8; 256];
    let n = sys_read(fd, buf.as_mut_ptr(), 256);
    sys_close(fd);
    if n <= 0 { return (host, port, index, index_len); }

    let cfg = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    for line in cfg.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(eq) = line.find('=') {
            let k = line[..eq].trim();
            let v = line[eq+1..].trim();
            match k {
                "host" => {
                    // "10.0.2.2" → [10, 0, 2, 2]
                    let mut p = [0u8; 4];
                    let mut idx = 0;
                    let mut cur: u32 = 0;
                    let mut any = false;
                    for &b in v.as_bytes() {
                        if b == b'.' {
                            if idx < 4 { p[idx] = cur as u8; }
                            idx += 1; cur = 0;
                        } else if b >= b'0' && b <= b'9' {
                            cur = cur * 10 + (b - b'0') as u32;
                            any = true;
                        }
                    }
                    if idx < 4 && any { p[idx] = cur as u8; }
                    if idx == 3 { host = p; }
                }
                "port" => {
                    let mut n: u32 = 0;
                    for &b in v.as_bytes() {
                        if b >= b'0' && b <= b'9' { n = n * 10 + (b - b'0') as u32; }
                    }
                    if n > 0 && n < 65536 { port = n as u16; }
                }
                "index" => {
                    let bytes = v.as_bytes();
                    let copy = core::cmp::min(bytes.len(), index.len());
                    for i in 0..copy { index[i] = bytes[i]; }
                    index_len = copy;
                }
                _ => {}
            }
        }
    }
    (host, port, index, index_len)
}

/// Print one IP octet without leading zeros.
unsafe fn put_u8(n: u8) {
    if n >= 100 {
        uart::putc(b'0' + n / 100);
        uart::putc(b'0' + (n / 10) % 10);
    } else if n >= 10 {
        uart::putc(b'0' + n / 10);
    }
    uart::putc(b'0' + n % 10);
}

unsafe fn put_ip(ip: [u8; 4]) {
    put_u8(ip[0]); uart::putc(b'.');
    put_u8(ip[1]); uart::putc(b'.');
    put_u8(ip[2]); uart::putc(b'.');
    put_u8(ip[3]);
}

unsafe fn do_update_and_search() {
    use kuberve_libc::http_get;

    // Wait for DHCP + ARP to settle. EL0 kv starts in parallel with
    // kmain's graphics/keyboard/mouse init, and DHCP runs *after*
    // those, so the first connect fails with "SYN send FAILED" —
    // http.rs retry masks it but wastes 1 second.
    sys_sleep(2);

    uart::puts(b"kv: --- update ---\n");

    // Ensure /packages exists (idempotent)
    let _ = sys_mkdir("/packages");

    let (host, port, index_buf, index_len) = read_repo_cfg();
    let index_path = core::str::from_utf8(&index_buf[..index_len]).unwrap_or("/index.txt");

    uart::puts(b"kv: repo ");
    put_ip(host);
    uart::puts(b" idx=");
    uart::puts(index_path.as_bytes());
    uart::puts(b"\n");

    match http_get(host, port, index_path) {
        Ok(body) => {
            uart::puts(b"kv: got ");
            print_u64(body.len() as u64);
            uart::puts(b" bytes\n");

            // Save to /packages/index.txt
            let fd = sys_open("/packages/index.txt", flags::O_CREAT);
            if fd < 0 {
                uart::puts(b"kv: cannot save index\n");
                return;
            }
            let w = sys_write_file(fd, body.as_ptr(), body.len() as u64);
            sys_close(fd);
            if w < 0 {
                uart::puts(b"kv: write FAILED\n");
                return;
            }

            // Count non-comment lines
            let mut n = 0u32;
            for line in core::str::from_utf8(body).unwrap_or("").lines() {
                let l = line.trim();
                if !l.is_empty() && !l.starts_with('#') { n += 1; }
            }
            uart::puts(b"kv: ");
            print_u64(n as u64);
            uart::puts(b" packages available\n");
        }
        Err(e) => {
            uart::puts(b"kv: update FAILED: ");
            uart::puts(e.as_str().as_bytes());
            uart::puts(b"\n");
            return;
        }
    }

    // ========== search "hello" ==========
    uart::puts(b"kv: --- search hello ---\n");

    let ifd = sys_open("/packages/index.txt", 0);
    if ifd < 0 {
        uart::puts(b"kv: index missing\n");
        return;
    }
    let mut ibuf = [0u8; 2048];
    let n = sys_read(ifd, ibuf.as_mut_ptr(), 2048);
    sys_close(ifd);
    if n <= 0 {
        uart::puts(b"kv: index empty\n");
        return;
    }

    let content = core::str::from_utf8(&ibuf[..n as usize]).unwrap_or("");
    let mut found = 0u32;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(eq) = line.find('=') {
            let name = line[..eq].trim();
            let ver = line[eq+1..].trim();
            if name.contains("hello") {
                uart::puts(b"  ");
                uart::puts(name.as_bytes());
                uart::puts(b"  ");
                uart::puts(ver.as_bytes());
                uart::puts(b"\n");
                found += 1;
            }
        }
    }
    uart::puts(b"kv: found ");
    print_u64(found as u64);
    uart::puts(b" match(es)\n");
}