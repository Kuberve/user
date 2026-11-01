[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange.svg)]()
[![Platform: aarch64](https://img.shields.io/badge/platform-aarch64-lightgrey.svg)]()

# Kuberve Userspace

Userspace programs for **Kuberve** — an independent operating system for
aarch64. Written in Rust (`no_std`), compiled to `aarch64-unknown-none`,
executed in EL0 by the Kuberve kernel.

> **Pre-alpha.** ABI is unstable. Programs compiled against one version of
> `kuberve-libc` may not run on a kernel from another version.

## Programs

| Binary | Path | Purpose |
|---|---|---|
| `init` | `/system/bin/init` | PID 1. Reads `/system/conf/init.cfg`, spawns services. |
| `uart` | `/system/bin/uart` | UART driver. Handles IRQ 33, echoes input. |
| `tcp` | `/system/bin/tcp` | TCP client test. Connects to `10.0.2.2:8080` (QEMU host). |
| `kv` | `/system/bin/kv` | Package manager. `install`/`list`/`remove` `.kvpkg` archives. |
| `hw` | `/system/bin/hw` | Hello-window test. Creates a GUI window via `SYS_GUI_CREATE_WINDOW`. |
| `user` | (legacy) | Pre-`init` build. Kept for `include_bytes!` fallback. |

## Prerequisites

- Rust nightly
- `aarch64-unknown-none` target: `rustup target add aarch64-unknown-none`

## Build

The userspace programs depend on `kuberve-libc` via a `git` dependency (pulled automatically by Cargo).

```bash
git clone https://github.com/Kuberve/user.git
cd user
cargo build --release
```

Binaries are produced in `target/aarch64-unknown-none/release/`:
`init`, `uart`, `tcp`, `kv`, `hw`, `user`.

## Deploy to FAT32 image

The kernel mounts `nvme.img` (FAT32) and loads ELF files from
`/system/bin/`. Copy binaries:

```bash
# Assuming you are in the kernel directory where nvme.img resides
for bin in init uart tcp kv hw; do
  mcopy -o -i nvme.img ../user/target/aarch64-unknown-none/release/$bin ::/system/bin/$bin
done
```

**FAT32 8.3 constraint:** file names must be ≤ 8 chars (no extension) or
≤ 8+3. `manifest.txt` → `MANIFEST.TXT`, but `long_name` → `LONG_NA~1` will
**not** be found by `find_entry_by_path`. See
[Kuberve/spec → fs/layout.md](https://github.com/Kuberve/spec/blob/main/fs/layout.md).

## Writing a program

See `src/bin/hw.rs` for a minimal example. Skeleton:

```rust
#![no_std]
#![no_main]

use core::arch::global_asm;
use kuberve_libc::{sys_exit, sys_sleep, uart};

global_asm!(
    ".section .text._start",
    ".global _start",
    "_start:",
    "  bl user_main",
    "  mov x0, #0",
    "  mov x8, #2",       // SYS_EXIT
    "  svc #0",
    "1: b 1b",
);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_exit(1);
}

#[no_mangle]
pub extern "C" fn user_main() -> ! {
    uart::puts(b"hello from EL0\n");
    loop { sys_sleep(1); }
}
```

Add `[[bin]]` to `Cargo.toml`, build, `mcopy` to `/system/bin/<name>`, then
either `sys_spawn("/system/bin/<name>", 2)` from another process or add it
to `/system/conf/init.cfg`:

```
/system/bin/<name>  2
```

Format: `<absolute_path> <priority>` per line. `#` starts a comment.

## Configuration

`/system/conf/init.cfg`:
```
# comment
/system/bin/uart  2
/system/bin/hw    2
```

`init` reads this file on boot and spawns each listed service.

## Related repos (Kuberve Org)

- [kuberve-abi](https://github.com/Kuberve/abi) — syscall numbers, `Message` (MIT)
- [kuberve-libc](https://github.com/Kuberve/libc) — syscall wrappers, heap, UART MMIO (LGPLv3)
- [kuberve-kernel](https://github.com/Kuberve/kernel) — the kernel (AGPLv3)
- [kuberve-docs](https://github.com/Kuberve/docs) — architecture, roadmap, devlog
- [kuberve-spec](https://github.com/Kuberve/spec) — formal ABI specs

## Contributing

Pull requests are accepted, but may take time to review. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) and sign the [CLA](CLA.md) before opening
a PR.

## License

- **Userspace programs:** AGPL-3.0-or-later. See [LICENSE](LICENSE).
- **Commercial licensing:** available for closed-source use.
  See [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md).
- **Trademark:** "Kuberve" is a trademark. See [TRADEMARK.md](TRADEMARK.md).
- **FAQ:** [LICENSE-FAQ.md](LICENSE-FAQ.md).

---

**Kuberve** is a trademark of the project author. The source code is licensed
under AGPLv3, but the name and logo may not be used to endorse derivative
products without written permission.
