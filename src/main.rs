#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::{asm, global_asm};
use core::alloc::Layout;

use kuberve_libc::{
    Message,
    BumpAlloc,
    sys_exit, sys_getpid, sys_send, sys_recv, sys_irq_ack,
    uart,
};
use kuberve_libc::{print, println};

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
    kuberve_libc::sys_exit(2);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    kuberve_libc::sys_write(1, b"panic!\n".as_ptr(), 7);
    kuberve_libc::sys_exit(1);
}

// --- UART server loop (userspace driver) ---
fn uart_server_loop() -> ! {
    uart::puts(b"uart_server up, pid=2\n");
    uart::rx_init();
    sys_irq_ack(33);

    loop {
        let mut msg = Message { tag: 0, data: [0; 7] };
        sys_recv(&mut msg);
        match msg.tag {
            1 => {
                // WRITE: kernel уже упаковал байты в data[1..], data[0] = len
                let bytes = kuberve_libc::write_payload(&msg);
                for &c in bytes {
                    if c == b'\n' { uart::putc(b'\r'); }
                    uart::putc(c);
                }
            }
            2 => {
                // IRQ
                while let Some(c) = uart::try_read() {
                    if c == b'\r' || c == b'\n' { uart::putc(b'\n'); }
                    else { uart::putc(c); }
                }
                sys_irq_ack(33);
            }
            _ => {}
        }
    }
}

fn client_hello(uart_pid: u64, text: &'static [u8]) -> ! {
    for _ in 0..2 {
        let msg = Message {
            tag: 1,
            data: [text.as_ptr() as u64, text.len() as u64, 0, 0, 0, 0, 0],
        };
        sys_send(uart_pid, &msg);
    }
    sys_exit(0);
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    let pid = sys_getpid();
    match pid {
        2 => uart_server_loop(),
        3 => {
            use kuberve_libc::flags;

            println!("[pid=3] UDP test: DNS query to 10.0.2.3:53");

            let fd = kuberve_libc::sys_socket(2, 2);   // AF_INET, SOCK_DGRAM
            if fd < 0 { println!("[pid=3] socket failed"); sys_exit(1); }
            println!("[pid=3] socket fd={}", fd);

            let r = kuberve_libc::sys_bind(fd, 5555);
            if r < 0 { println!("[pid=3] bind failed"); sys_exit(1); }
            println!("[pid=3] bound to :5555");

            // DNS query для "a.com" A record
            let mut query = [0u8; 32];
            query[0] = 0x12; query[1] = 0x34;   // ID
            query[2] = 0x01; query[3] = 0x00;   // flags: standard, RD
            query[4] = 0x00; query[5] = 0x01;   // QDCOUNT = 1
            // rest zeros
            // QNAME: "a" "com" ""
            query[12] = 1; query[13] = b'a';
            query[14] = 3; query[15] = b'c'; query[16] = b'o'; query[17] = b'm';
            query[18] = 0;
            // QTYPE = 1 (A), QCLASS = 1 (IN)
            query[19] = 0; query[20] = 1;
            query[21] = 0; query[22] = 1;
            let query_len = 23;

            // 10.0.2.3 → u32: (10<<24)|(0<<16)|(2<<8)|3 = 0x0A000203
            let dst_ip: u32 = 0x0A000203;
            let n = kuberve_libc::sys_sendto(fd, query.as_ptr(), query_len as u64, dst_ip, 53);
            if n < 0 {
                println!("[pid=3] sendto failed: {}", n);
            } else {
                println!("[pid=3] sent {} bytes to 10.0.2.3:53", n);
            }

            // Ждём reply (polling + yield, чтобы kernel poll_rx успел)
            let mut rbuf = [0u8; 512];
            let mut reply_received = false;
            for attempt in 0..1000 {
                let mut src_ip = [0u8; 4];
                let mut src_port: u16 = 0;
                let r = kuberve_libc::sys_recvfrom(
                    fd, rbuf.as_mut_ptr(), 512,
                    src_ip.as_mut_ptr(), &mut src_port as *mut u16);
                if r > 0 {
                    print!("[pid=3] UDP reply: ");
                    for i in 0..r.min(23) { print!("{:02x} ", rbuf[i as usize]); }
                    println!();
                    println!("[pid=3] UDP reply from {}.{}.{}.{}:{} len={}",
                             src_ip[0], src_ip[1], src_ip[2], src_ip[3], src_port, r);
                    reply_received = true;
                    break;
                }
                if attempt % 100 == 0 {
                    kuberve_libc::sys_sleep(1);   // 1 тик = 1 сек, даём сети время
                } else {
                    kuberve_libc::sys_yield();
                }
            }
            if !reply_received {
                println!("[pid=3] no UDP reply (timeout)");
            }
            kuberve_libc::sys_close(fd);

            sys_exit(0);
        }
        4 => client_hello(2, b"client2 says hi via IPC\n"),
        _ => sys_exit(1),
    }
}