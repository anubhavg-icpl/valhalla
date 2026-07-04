<p align="center">
  <img src="docs/assets/images/hero-banner.png" alt="Valhalla - Rust Windows Kernel Monitor" width="720"/>
</p>

<h1 align="center">Valhalla</h1>

<p align="center">
  A lightweight, <strong>Rust-native</strong> Windows kernel-mode monitoring driver and companion user-mode client that captures process, thread, image-load, and registry events in real time.
</p>

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-BSD--3--Clause-blue.svg"/>
  <img alt="Rust" src="https://img.shields.io/badge/rust-nightly-orange.svg"/>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%20x64-lightgrey.svg"/>
  <img alt="Driver" src="https://img.shields.io/badge/type-kernel--mode%20driver-red.svg"/>
  <img alt="Status" src="https://img.shields.io/badge/status-experimental-yellow.svg"/>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> &bull;
  <a href="#architecture">Architecture</a> &bull;
  <a href="#build-from-source">Build</a> &bull;
  <a href="#installation">Install</a> &bull;
  <a href="#usage">Usage</a> &bull;
  <a href="#troubleshooting">Troubleshooting</a> &bull;
  <a href="#contributing">Contributing</a> &bull;
  <a href="#acknowledgments">Acknowledgments</a>
</p>

---

## Overview

**Valhalla** is a research-grade Windows kernel monitoring tool written entirely in Rust. It loads as a signed kernel-mode driver (`valhalla.sys`), registers a suite of kernel notification callbacks, and exposes a device object that a user-mode client (`valhalla-client.exe`) reads from to surface security-relevant system activity.

Valhalla is inspired by Pavel Yosifovich's `SysMon` sample from the book *Windows Kernel Programming*, but is rewritten from scratch in idiomatic Rust using the `windows-kernel-rs` bindings, `no_std` allocation, and Rust's type system to enforce safer kernel programming patterns.

> :warning: **Valhalla is experimental software.** It is intended for education, research, and blue-team telemetry prototyping on isolated virtual machines. Do not deploy on production systems.

### Why Rust for a kernel driver?

| Concern | C / WDM tradition | Valhalla (Rust) |
|---|---|---|
| Memory safety | Manual, error-prone `ExAllocatePool`/`RtlCopyMemory` | Borrow-checked `alloc::Vec`, `core::ptr::copy_nonoverlapping` |
| String handling | `UNICODE_STRING` boilerplate, buffer overruns | `kernel-string` wrapper with `as_rust_string()` |
| Concurrency | Raw `KSPIN_LOCK`, `FAST_MUTEX` primitives | `kernel-fast-mutex` with RAII `AutoLock` guards |
| Cleanup on error | Goto-based cleanup, leaked callbacks | `Cleaner` struct with pattern-based teardown |
| Pattern matching | `if`/`switch` ladders | `enum ItemInfo` with exhaustive `match` |
| Crash safety | One NULL deref = BSOD | `Option`, `Result`, `null_mut()` checks enforced |

Rust's ownership model does not eliminate kernel-mode hazards (you can still panic the kernel, deadlock, or corrupt shared state through `unsafe`), but it raises the floor considerably versus hand-rolled C.

### What Valhalla monitors

Valhalla registers **four** kernel notification mechanisms and surfaces a tagged event for each:

| Event class | Kernel API | `ItemInfo` variant | What it captures |
|---|---|---|---|
| Process create / exit | `PsSetCreateProcessNotifyRoutineEx` | `ProcessCreate`, `ProcessExit` | PID, parent PID, image path (truncated to 64 bytes) |
| Thread create / exit | `PsSetCreateThreadNotifyRoutine` | `ThreadCreate`, `ThreadExit` | Owning PID, TID |
| Image load | `PsSetLoadImageNotifyRoutine` | `ImageLoad` | PID, base address, image size, file name |
| Registry value set | `CmRegisterCallbackEx` | `RegistrySetValue` | PID, TID, key name, data type (HKLM only) |

Events are buffered in a kernel ring (capacity 256 events) and drained by the user-mode client via a single `ReadFile` against the `\\.\Valhalla` symbolic link.

---

## Table of contents

- [Overview](#overview)
  - [Why Rust for a kernel driver?](#why-rust-for-a-kernel-driver)
  - [What Valhalla monitors](#what-valhalla-monitors)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
  - [Workspace layout](#workspace-layout)
  - [Kernel-mode driver (`valhalla-km`)](#kernel-mode-driver-valhalla-km)
  - [User-mode client (`valhalla-um`)](#user-mode-client-valhalla-um)
  - [Shared protocol (`common`)](#shared-protocol-common)
  - [Build orchestrator (`xtask`)](#build-orchestrator-xtask)
  - [Data flow](#data-flow)
  - [IRP dispatch table](#irp-dispatch-table)
  - [Synchronization model](#synchronization-model)
  - [Memory layout](#memory-layout)
- [Prerequisites](#prerequisites)
- [Build from source](#build-from-source)
  - [1. Clone](#1-clone)
  - [2. Configure the toolchain](#2-configure-the-toolchain)
  - [3. Build the client](#3-build-the-client)
  - [4. Build the driver](#4-build-the-driver)
  - [5. Sign the driver](#5-sign-the-driver)
  - [6. Verify the artifacts](#6-verify-the-artifacts)
- [Installation](#installation)
  - [Test signing prerequisites](#test-signing-prerequisites)
  - [Loading the driver](#loading-the-driver)
  - [Unloading the driver](#unloading-the-driver)
- [Usage](#usage)
  - [Reading events](#reading-events)
  - [Sample output](#sample-output)
  - [Event reference](#event-reference)
  - [Buffering semantics](#buffering-semantics)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)
- [Security considerations](#security-considerations)
- [Development](#development)
  - [Formatting and linting](#formatting-and-linting)
  - [Running the xtask](#running-the-xtask)
  - [Project conventions](#project-conventions)
- [Roadmap](#roadmap)
- [FAQ](#faq)
- [Contributing](#contributing)
- [Acknowledgments](#acknowledgments)

---

## Quick Start

The fastest way to see Valhalla in action. Assumes Windows 10/11 x64 with admin rights and test signing enabled on a VM.

```powershell
# 1. Clone
git clone https://github.com/anubhavg-icpl/valhalla.git
cd valhalla

# 2. Make sure you are on the nightly toolchain (auto-selected via rust-toolchain.toml)
rustup toolchain install nightly-x86_64-pc-windows-msvc

# 3. Build everything (driver .dll -> .sys + client .exe)
cargo build --release

# 4. Build & sign the driver via xtask (requires Visual Studio BuildTools + WDK)
cargo xtask driver

# 5. Enable test signing (one-time, requires reboot)
bcdedit.exe -set TESTSIGNING ON

# 6. Install and start the driver service (admin shell)
sc create valhalla type= kernel binPath= "D:\valhalla\target\release\valhalla.sys"
sc start valhalla

# 7. In another shell, run the client to drain events
.\target\release\valhalla-client.exe

# 8. When done
sc stop valhalla
sc delete valhalla
```

You should see live process, thread, image-load, and HKLM registry events printed to the console.

---

## Architecture

Valhalla is a Cargo workspace composed of four crates. The high-level shape is intentionally close to a textbook WDM driver, with Rust idioms layered on top.

<p align="center">
  <img src="docs/assets/images/architecture-diagram.png" alt="Valhalla architecture diagram" width="860"/>
</p>

### Workspace layout

```
valhalla/
|-- Cargo.toml                 # Workspace manifest (resolver = "2")
|-- rust-toolchain.toml        # Pins nightly-x86_64-pc-windows-msvc
|-- rustfmt.toml               # Format settings
|-- .cargo/
|   `-- config.toml             # cargo xtask alias
|-- common/                     # Shared, no_std event protocol crate
|   |-- Cargo.toml
|   `-- src/
|       `-- lib.rs              # ItemInfo enum, StringBuff, serialization helpers
|-- valhalla-km/                # Kernel-mode driver crate
|   |-- Cargo.toml              # name = "valhalla", crate-type = ["cdylib"]
|   |-- build.rs                # Locates WDK lib paths via winreg
|   |-- .cargo/config.toml      # Linker flags for /DRIVER, /ENTRY:DriverEntry
|   |-- rust-toolchain.toml     # (legacy per-crate nightly pin)
|   |-- DriverCertificate.cer   # Test-signing cert (public part only)
|   `-- src/
|       |-- lib.rs              # DriverEntry, IRP dispatch, callbacks, ring buffer
|       |-- cleaner.rs          # RAII-style rollback for partial init failures
|       `-- ioctl_code.rs       # Custom IOCTL codes
|-- valhalla-um/                # User-mode client crate
|   |-- Cargo.toml              # name = "valhalla-client"
|   `-- src/
|       |-- main.rs             # Opens \\.\Valhalla, ReadFile loop, prints events
|       `-- error_msg.rs        # FormatMessageW wrapper for Win32 errors
`-- xtask/                      # Cargo-powered build orchestrator
    |-- Cargo.toml
    `-- src/
        `-- main.rs             # cargo xtask driver|client|clean|sign
```

### Kernel-mode driver (`valhalla-km`)

`valhalla-km` is the heart of Valhalla. It compiles to a `cdylib` that the MSVC linker turns into `valhalla.dll`, which `xtask` renames to `valhalla.sys` (the canonical kernel-driver extension). The crate is `#![no_std]` and pulls in `extern crate alloc` for heap collections.

**Entry point** (`DriverEntry`)

The I/O Manager calls `DriverEntry` once on load. It performs, in order:

1. Initialize the in-kernel logger (`KernelLogger::init`) at `LevelFilter::Trace`.
2. Initialize the global `FastMutex` that guards the event ring.
3. Pre-allocate the event `Vec<ItemInfo>` with `try_reserve_exact(256)`. On `Err`, return `STATUS_INSUFFICIENT_RESOURCES` immediately.
4. Wire up `DRIVER_OBJECT.MajorFunction[]` for `CREATE`, `CLOSE`, `DEVICE_CONTROL`, `READ`, `WRITE`.
5. Set `DriverUnload` so the driver can be cleanly stopped via `sc stop`.
6. Use the `loop { ... break; }` pattern (a Rust-flavored goto) to run a sequence of init steps. Each step records a cleanup hook in a `Cleaner`. If any step fails, `cleaner.clean()` rolls back everything that succeeded.

The init sequence registers:

| Step | API | Cleaner hook |
|---|---|---|
| Create device `\Device\Valhalla` | `IoCreateDevice` | `IoDeleteDevice` |
| Create symlink `\??\Valhalla` | `IoCreateSymbolicLink` | `IoDeleteSymbolicLink` |
| Process notify | `PsSetCreateProcessNotifyRoutineEx` | `PsSetCreateProcessNotifyRoutineEx(.., TRUE)` (remove) |
| Thread notify | `PsSetCreateThreadNotifyRoutine` | `PsRemoveCreateThreadNotifyRoutine` |
| Image load notify | `PsSetLoadImageNotifyRoutine` | `PsRemoveLoadImageNotifyRoutine` |
| Registry callback | `CmRegisterCallbackEx` (altitude `7657.124`) | `CmUnRegisterCallback` |

The device is configured for `DO_DIRECT_IO`, which means read IRPs carry an `MDL` that the driver maps into system address space via `MmGetSystemAddressForMdlSafe`. This is the canonical WDM pattern for safely touching user buffers from kernel.

**Callbacks**

Each notification callback constructs an `ItemInfo` variant and pushes it into the ring via `push_item_thread_safe`, which takes the global `FastMutex` through an RAII `AutoLock`. The ring is bounded: when it reaches `MAX_ITEM_COUNT = 256`, the oldest event is dropped (`events.remove(0)`). This favors fresh events over old ones and bounds kernel memory usage.

The registry callback specifically filters for `REG_NT_POST_SET_VALUE_KEY` against keys under `\REGISTRY\MACHINE` (i.e. `HKEY_LOCAL_MACHINE`). Other hives (`HKCU`, `HKU`, etc.) are ignored to keep the signal-to-noise ratio high. The pre-operation info gives access to `ValueName`, `DataType`, and `Data`, though Valhalla currently surfaces only the key name and type.

**Read dispatch**

`DispatchRead` is the only path that hands data to user mode:

1. Pull the IRP stack location and read `Parameters.Read.Length`.
2. Reject zero-length reads with `STATUS_INVALID_BUFFER_SIZE`.
3. Map the IRP's MDL via `MmGetSystemAddressForMdlSafe(... NormalPagePriority)`.
4. Under the global lock, `copy_events_to_ptr` memcpy's the entire ring into the MDL buffer, **clears the ring**, and returns the byte count.
5. Complete the IRP with `STATUS_SUCCESS` and `Information = copied_bytes`.

This is a destructive drain: a successful read empties the kernel buffer. The client is expected to call `ReadFile` in a loop.

**Unload**

`DriverUnload` mirrors `DriverEntry` in reverse: delete the device, delete the symlink, unregister each callback. Because `DriverEntry` recorded everything in `Cleaner`, the same teardown logic is reused for both happy-path unload and partial-init failure.

### User-mode client (`valhalla-um`)

The client is a small CLI binary (`valhalla-client.exe`) whose only job is to open `\\.\Valhalla`, issue a single 64 KiB `ReadFile`, and pretty-print each `ItemInfo` it receives. It deliberately does not loop, poll, or filter; it is a smoke-test client that proves the kernel -> user pipe works. Production telemetry consumers would replace this with a proper event loop that reconnects on failure, batches reads, and ships events to a SIEM.

Error reporting uses `FormatMessageW` to turn `GetLastError()` codes into human-readable strings, surfaced via `print_last_error`.

### Shared protocol (`common`)

The `common` crate is `#![no_std]` and `extern crate alloc`, which lets the kernel driver depend on it without dragging in `std`. It defines:

```rust
pub const BUFF_SIZE: usize = 64;

pub struct StringBuff([u8; BUFF_SIZE]);

#[repr(C)]
pub enum ItemInfo {
    ProcessCreate   { pid: u32, parent_pid: u32, command_line: StringBuff },
    ProcessExit     { pid: u32 },
    ThreadCreate    { pid: u32, tid: u32 },
    ThreadExit      { pid: u32, tid: u32 },
    ImageLoad       { pid: u32, load_address: isize, image_size: usize, image_file_name: StringBuff },
    RegistrySetValue{ pid: u32, tid: u32, key_name: StringBuff, data_type: u32 },
}
```

`StringBuff` is a fixed-size 64-byte buffer that avoids variable-length allocation across the kernel/user boundary. Strings longer than 63 bytes are truncated. The `#[repr(C)]` enum guarantees a stable, C-compatible layout so the driver and the client can reinterpret the same bytes.

`ItemInfo::string_to_buffer` is the kernel-side helper that copies a Rust `String` into a `StringBuff` with truncation, used by every callback.

### Build orchestrator (`xtask`)

Valhalla follows the [cargo xtask pattern](https://github.com/matklad/cargo-xtask). The `.cargo/config.toml` alias lets you invoke `cargo xtask <task>` which transparently runs `cargo run -p xtask -- <task>`. Available tasks:

| Task | What it does |
|---|---|
| `cargo xtask client` | `cargo build --release -p valhalla-client` |
| `cargo xtask driver` | Builds `valhalla`, renames `.dll` to `.sys`, then calls `sign` |
| `cargo xtask sign`   | Invokes `signtool` against `target\release\valhalla.sys` using the `DriverCertificate` from `PrivateCertStore` |
| `cargo xtask clean`  | Removes the `target/` directory |

The `sign` task shells out to `vcvars64.bat` to bring the Visual Studio toolchain into scope, then runs `signtool sign /fd SHA256 /a /v /s PrivateCertStore /n DriverCertificate /t http://timestamp.digicert.com`. It currently relies on a pre-existing `DriverCertificate` in `certutil`'s `PrivateCertStore`; producing that cert is documented under [Build from source](#5-sign-the-driver).

### Data flow

<p align="center">
  <img src="docs/assets/images/data-flow.png" alt="Valhalla data flow" width="780"/>
</p>

```
  +-------------------------+         callback          +-----------------------+
  | Kernel notification API | ----------------------->  | Valhalla callback fn  |
  |  (Ps/Cm* family)        |                           |  constructs ItemInfo  |
  +-------------------------+                           +-----------+-----------+
                                                                    |
                                                     push_item_thread_safe (under G_MUTEX)
                                                                    |
                                                                    v
                                          +---------------------------------------------+
                                          |  G_EVENTS: Vec<ItemInfo>  (cap 256, ring)   |
                                          +-----------------------+---------------------+
                                                                  |
                                                  ReadFile (DO_DIRECT_IO / MDL)
                                                                  |
                                                                  v
                                          +---------------------------------------------+
                                          |  valhalla-client.exe                        |
                                          |  CreateFileA("\\\\.\\Valhalla", READ)       |
                                          |  ReadFile -> display_info -> println!       |
                                          +---------------------------------------------+
```

### IRP dispatch table

| `IRP_MJ_*` | Handler | Behavior |
|---|---|---|
| `CREATE` | `DispatchCreateClose` | Complete with `STATUS_SUCCESS` |
| `CLOSE`  | `DispatchCreateClose` | Complete with `STATUS_SUCCESS` |
| `DEVICE_CONTROL` | `DispatchDeviceControl` | If `IoControlCode == IOCTL_REQUEST`, log; otherwise reject with `STATUS_INVALID_DEVICE_REQUEST` |
| `READ` | `DispatchRead` | Drain ring into MDL buffer |
| `WRITE` | `DispatchWrite` | No-op, completes with the requested length |

### Synchronization model

Valhalla uses a single global `FastMutex` (`G_MUTEX`) to serialize all ring access from both the notification callbacks and the read IRP path. `FastMutex` is appropriate because:

- The critical sections are short (a `Vec::push`, a `ptr::copy_nonoverlapping`, or a `Vec::clear`).
- None of the work is pageable while the lock is held.
- Callbacks run at `PASSIVE_LEVEL` or `APC_LEVEL`, which is compatible with `FAST_MUTEX` (which raises IRQL to `APC_LEVEL`).

An `AutoLock` RAII guard acquires on construction and releases on drop, so early `return` from any path releases the lock automatically. There is currently no per-CPU sharding or lock-free ring; a future revision could use a `SEQUENCE`-guarded single-producer-multi-consumer ring for higher throughput.

### Memory layout

| Object | Where it lives | Lifetime |
|---|---|---|
| `G_EVENTS: Option<Vec<ItemInfo>>` | Non-paged kernel heap (via `alloc`) | Driver lifetime |
| `G_MUTEX: FastMutex` | Static | Driver lifetime |
| `G_COOKIE: LARGE_INTEGER` | Static | Driver lifetime |
| `StringBuff` (inside `ItemInfo`) | Inline in the `Vec` element | Owned by the ring |
| Read buffer | Caller's MDL, mapped via `MmGetSystemAddressForMdlSafe` | Per-IRP |

All kernel allocations come from the non-paged pool implicitly through Rust's `alloc` (the `kernel-init` crate installs a custom `#[global_allocator]` and `#[alloc_error_handler]` that route to `ExAllocatePoolWithTag`). This means a `Vec::push` that fails to allocate will trigger the kernel's allocation-error handler and abort the operation safely, rather than panicking in the kernel.

---

## Prerequisites

Valhalla is Windows-only and requires a fairly specific toolchain. The build has been validated on Windows 10/11 x64.

### Required

| Component | Version | Why |
|---|---|---|
| **Rust nightly** | `nightly-x86_64-pc-windows-msvc` | Required by `km-api-sys` for `#![feature(...)]` attributes; pinned via `rust-toolchain.toml` |
| **Windows SDK** | 10.0.18362+ | Headers and import libs referenced by `build.rs` |
| **Windows Driver Kit (WDK)** | Matching the SDK | Provides `km\` libraries (`ntoskrnl.lib`, etc.) |
| **Visual Studio Build Tools 2019+** | C++ build tools | `link.exe`, `vcvars64.bat` for signing |
| **PowerShell 7+** | Any modern build | Shell used by these docs |

### Optional

| Component | Use |
|---|---|
| **VirtualBox / VMware / Hyper-V VM** | Strongly recommended; never install a test-signed driver on bare metal that you care about |
| **WinDbg** (Preview or Classic) | Kernel debugging the driver if it does not load |
| **DriverView** / **Poolmon** | Inspecting loaded drivers and non-paged pool usage |

### Why nightly?

The upstream `windows-kernel-rs` bindings used by Valhalla (`km-api-sys`, `kernel-string`, `kernel-macros`, `kernel-fast-mutex`, `kernel-init`) gate some of their implementation behind nightly-only `#![feature(...)]` attributes. Until those stabilize, Valhalla must be built on nightly. The `rust-toolchain.toml` at the workspace root pins this automatically; `cargo build` will select nightly for you.

---

## Build from source

### 1. Clone

```powershell
git clone https://github.com/anubhavg-icpl/valhalla.git
cd valhalla
```

### 2. Configure the toolchain

The repo ships a `rust-toolchain.toml` that pins `nightly-x86_64-pc-windows-msvc`. If you do not yet have it installed:

```powershell
rustup toolchain install nightly-x86_64-pc-windows-msvc
rustup component add rust-src --toolchain nightly-x86_64-pc-windows-msvc
rustup component add rustfmt clippy --toolchain nightly-x86_64-pc-windows-msvc
```

Verify:

```powershell
PS> cargo --version
cargo 1.xx.0-nightly (...)
PS> rustc +nightly --version
rustc 1.xx.0-nightly (...)
```

### 3. Build the client

The client is plain Rust and needs nothing special:

```powershell
cargo build --release -p valhalla-client
# or
cargo xtask client
```

The binary will appear at `target\release\valhalla-client.exe`.

### 4. Build the driver

The driver is a `cdylib` whose `.cargo/config.toml` instructs the linker to emit a kernel-mode binary. Build it from the workspace root:

```powershell
cargo build --release -p valhalla
```

The cargo artifact will be `target\release\valhalla.dll`. Rename it to `valhalla.sys` (the extension Windows expects for drivers):

```powershell
Move-Item target\release\valhalla.dll target\release\valhalla.sys
```

Or do both at once with the xtask:

```powershell
cargo xtask driver
```

### 5. Sign the driver

Windows will refuse to load an unsigned driver, even in test-signing mode. You must produce a self-signed cert and sign `valhalla.sys` with it.

**5a. Create the test certificate (one-time):**

```powershell
# Run from an elevated Developer Command Prompt for VS 2019+
makecert -r -pe -ss PrivateCertStore -n "CN=DriverCertificate" valhalla-km\DriverCertificate.cer
```

This stores the cert in `PrivateCertStore` and writes the public part to `valhalla-km\DriverCertificate.cer` (already present in the repo).

**5b. Sign the binary:**

```powershell
cargo xtask sign
```

This invokes `signtool` via `vcvars64.bat`:

```
signtool sign /fd SHA256 /a /v /s PrivateCertStore /n DriverCertificate /t http://timestamp.digicert.com target\release\valhalla.sys
```

**5c. Verify the signature:**

```powershell
signtool verify /v /pa target\release\valhalla.sys
```

### 6. Verify the artifacts

After a successful build you should have:

```
target\release\
|-- valhalla.sys           <- the driver (signed)
|-- valhalla.pdb           <- debug symbols for WinDbg
|-- valhalla-client.exe    <- the user-mode client
`-- valhalla-client.pdb
```

---

## Installation

### Test signing prerequisites

Test-signed drivers will only load if the target machine has test signing enabled.

```powershell
# Enable test signing (requires a reboot)
bcdedit.exe -set TESTSIGNING ON

# After reboot, you will see "Test Mode" watermark on the desktop.
# To disable later:
# bcdedit.exe -set TESTSIGNING OFF
```

If you skip this step, `sc start valhalla` will fail with `STATUS_INVALID_IMAGE_HASH` (error 577 / `0x241`).

### Loading the driver

```powershell
# Create the service (one-time)
sc create valhalla type= kernel binPath= "C:\path\to\valhalla.sys"

# Start it
sc start valhalla

# Query status
sc query valhalla
```

> Note the spacing in `type= kernel` and `binPath= "..."`. The `sc` CLI is whitespace-sensitive; the space after `=` is required, and there must not be a space before it.

If the driver started successfully, `sc query valhalla` should report `STATE: 4 RUNNING`.

### Unloading the driver

```powershell
# Stop
sc stop valhalla

# Remove the service
sc delete valhalla
```

If `sc stop` hangs, the driver is likely stuck in an IRP that never completes, or a callback is still firing. Rebooting is the safe recovery path; do not `taskkill` the SCM.

---

## Usage

### Reading events

Open an elevated shell on the same machine the driver is running on and invoke the client:

```powershell
.\target\release\valhalla-client.exe
```

The client opens `\\.\Valhalla`, issues a single 64 KiB `ReadFile`, and prints every `ItemInfo` it received. Each event is printed via Rust's `Debug` impl, which respects the `StringBuff` formatter (string fields appear as quoted, NUL-trimmed UTF-8).

To get a continuous stream, wrap the client in a loop:

```powershell
while ($true) { .\target\release\valhalla-client.exe; Start-Sleep -Milliseconds 100 }
```

### Sample output

```
Hello, world!
after!
CreateFile success!
Read success! Bytes: 368
ProcessCreate { pid: 4884, parent_pid: 7260, command_line: "\"C:\\Windows\\System32\\notepad.exe\"" }
ThreadCreate { pid: 4884, tid: 9216 }
ImageLoad { pid: 4884, load_address: 140727834132480, image_size: 2095104, image_file_name: "\Device\HarddiskVolume3\Windows\System32\notepad.exe" }
RegistrySetValue { pid: 4884, tid: 9216, key_name: "\REGISTRY\MACHINE\SOFTWARE\Microsoft\Windows\Notepad\Default", data_type: 1 }
ThreadExit { pid: 4884, tid: 9216 }
ProcessExit { pid: 4884 }
```

### Event reference

| Variant | Fields | Source callback |
|---|---|---|
| `ProcessCreate` | `pid`, `parent_pid`, `command_line` (truncated 64 bytes) | `OnProcessNotify` with non-null `create_info` |
| `ProcessExit` | `pid` | `OnProcessNotify` with null `create_info` |
| `ThreadCreate` | `pid`, `tid` | `OnThreadNotify` with `create == TRUE` |
| `ThreadExit` | `pid`, `tid` | `OnThreadNotify` with `create == FALSE` |
| `ImageLoad` | `pid`, `load_address`, `image_size`, `image_file_name` | `OnImageLoadNotify` |
| `RegistrySetValue` | `pid`, `tid`, `key_name`, `data_type` | `OnRegistryNotify` (`REG_NT_POST_SET_VALUE_KEY`, HKLM only) |

### Buffering semantics

- **Capacity:** 256 events in the kernel ring (`MAX_ITEM_COUNT`).
- **Eviction:** When the ring is full, `events.remove(0)` drops the oldest event and pushes the new one. This is O(n) in `Vec` but n is small and bounded.
- **Drain:** A successful `ReadFile` **empties** the ring (see `copy_events_to_ptr`). Events generated between two reads accumulate; events generated faster than they are drained will overwrite older ones once 256 is reached.
- **Partial reads:** If the user buffer is smaller than the live ring contents, `copy_events_to_ptr` returns 0 and the IRP completes with `STATUS_INSUFFICIENT_RESOURCES`. The client uses a 64 KiB buffer which fits ~1000+ events, far above the 256 cap, so this should not occur in practice.

---

## Configuration

Valhalla is currently not runtime-configurable; all tunables are compile-time constants in `valhalla-km/src/lib.rs`:

| Constant | Default | Effect |
|---|---|---|
| `DEVICE_NAME` | `\Device\Valhalla` | Kernel device object name |
| `SYM_LINK_NAME` | `\??\Valhalla` | Win32 symbolic link (`\\.\Valhalla`) |
| `MAX_ITEM_COUNT` | `256` | Ring capacity before eviction kicks in |
| `BUFF_SIZE` (in `common`) | `64` | Inline string buffer in `StringBuff` |

The registry callback altitude (`7657.124`) is hardcoded in `DriverEntry`. If you ship a production variant you must allocate a real altitude from Microsoft.

---

## Troubleshooting

### `sc start` fails with error 577 (`0x241`)

> The Windows cannot verify the digital signature for this file.

Test signing is not enabled. Run `bcdedit.exe -set TESTSIGNING ON` and reboot. Also confirm `valhalla.sys` is signed with `signtool verify /v /pa valhalla.sys`.

### `sc start` fails with error 1058 or hangs

The service is disabled, missing, or the path is wrong. Double-check `binPath`:

```powershell
sc qc valhalla
```

Make sure `BINARY_PATH_NAME` points to the actual `.sys` location. Paths with spaces must be quoted in the original `sc create`.

### `CreateFile` returns `INVALID_HANDLE_VALUE` from the client

The driver is not running, or the symbolic link was not created. Verify:

```powershell
sc query valhalla              # should be RUNNING
dir \\.\Valhalla               # should resolve
```

If `sc query` reports `RUNNING` but `\\.\Valhalla` does not resolve, the driver's `IoCreateSymbolicLink` likely failed. Check kernel debug output (via WinDbg or DbgView) for the `failed to create sym_link 0x...` log line.

### The client connects but reads return 0 bytes

Two causes:

1. **No events yet.** Generate activity (launch a program, set a value under `HKLM`). The ring only fills when callbacks fire.
2. **Buffer smaller than ring.** The client uses 64 KiB so this should not occur, but if you wrote your own consumer with a smaller buffer, `copy_events_to_ptr` will refuse and the ring will be retained.

### Blue screen (SYSTEM_SERVICE_EXCEPTION, etc.)

You are running an experimental kernel driver. If you hit a BSOD:

1. Note the bugcheck code and the failing module (it should be `valhalla.sys`).
2. Open the minidump in WinDbg and run `!analyze -v`.
3. The most likely culprits are:
   - A null `create_info.ImageFileName` dereference in `OnProcessNotify` (already guarded but worth re-checking).
   - The `Vec::remove(0)` path if `MAX_ITEM_COUNT` is ever reduced at runtime.
   - Lifetime issues with the registry callback's `PUNICODE_STRING` (we release with `CmCallbackReleaseKeyObjectIDEx`).

Recovery is to reboot into safe mode and `sc delete valhalla`.

### `cargo build` complains about `#![feature]`

You are not on nightly. The workspace `rust-toolchain.toml` pins nightly; if you have `RUSTUP_TOOLCHAIN` set in your environment, it will override the pin. Unset it:

```powershell
Remove-Item Env:\RUSTUP_TOOLCHAIN
```

### `clippy` errors on test target (duplicate lang item)

`cargo clippy` against the `--all-targets` flag will try to compile the `sysmon` lib's test harness, which conflicts with `kernel-init`'s `#[panic_impl]` and `#[alloc_error_handler]`. This is inherent to `no_std` kernel crates. Run clippy against libs and bins only:

```powershell
cargo clippy --release --lib --bins
```

---

## Security considerations

- **Test-signed only.** Valhalla ships with a self-signed cert in `PrivateCertStore`. Anyone with the private key (you, after `makecert`) can sign drivers that this machine will accept. Do not enable test signing on shared or internet-facing hosts.
- **No authentication on the device.** Any user-mode process that can open `\\.\Valhalla` can read all collected events. There is no ACL on the device object. In a real deployment you would set a security descriptor in `IoCreateDevice`'s `DeviceCharacteristics` or via `IoCreateDeviceSecure`.
- **No tamper protection.** A malicious admin could unload the driver, patch it, or simply not install it. Valhalla is a telemetry collector, not an EDR.
- **Ring eviction is lossy.** Under high event volume, old events are dropped. Do not rely on Valhalla as a complete audit log.
- **String truncation.** `StringBuff` truncates to 63 bytes. Long image paths or registry keys will be cut, which can affect matching logic in downstream consumers.

---

## Development

### Formatting and linting

Valhalla is held to `cargo fmt --all --check` and `cargo clippy --release --lib --bins` being clean. CI (when added) should enforce both.

```powershell
# Auto-format
cargo fmt --all

# Check format
cargo fmt --all --check

# Lint (libs + bins only; test target conflicts with no_std kernel crates)
cargo clippy --release --lib --bins -- -D warnings
```

`rustfmt.toml` opts into nightly-only features (`imports_granularity`, `format_strings`), which is why `cargo fmt` requires nightly. The workspace `rust-toolchain.toml` handles this automatically.

### Running the xtask

```powershell
cargo xtask           # prints help
cargo xtask client    # build user-mode client
cargo xtask driver    # build + rename + sign driver
cargo xtask sign      # sign existing valhalla.sys
cargo xtask clean     # wipe target/
```

### Project conventions

- **Edition 2021** across all crates.
- **Workspace resolver = "2"** to avoid the legacy feature unification pitfalls.
- **`#![no_std]`** in `common` and `valhalla-km`; `std` is allowed only in `valhalla-um` and `xtask`.
- **Kernel statics** use `static mut` with the `#![allow(static_mut_refs)]` escape hatch. All access is funneled through `AutoLock`-guarded helpers; do not add new direct references to `G_EVENTS` or `G_MUTEX` outside a lock.
- **Cleanup pattern:** every successful kernel registration call must be paired with a `Cleaner::init_*` so that a later failure rolls it back. New callbacks should follow this contract.
- **String handling across the kernel/user boundary:** use `StringBuff` and `ItemInfo::string_to_buffer`, never raw `String` or `Vec<u8>` in a `#[repr(C)]` struct.

---

## Roadmap

The repo's README keeps an aspirational roadmap; the actively-tracked items are:

- [ ] **Continuous read loop in the client.** Today the client reads once and exits. A proper event loop with reconnection belongs in `valhalla-um`.
- [ ] **Unit tests + mock framework.** The kernel crate is hard to test in-process, but the `common` protocol and the `Cleaner` state machine can be unit-tested in user mode.
- [ ] **GitHub Actions CI.** Run `cargo fmt --check`, `cargo clippy`, and `cargo build --release -p valhalla-client` on every push. (Driver build requires the WDK, which is heavier.)
- [ ] **Migrate to official `windows` / `windows-sys` crates** for the kernel-mode surface, once they grow one. Currently Valhalla depends on `winapi-rs` (feature/km branch) and `windows-kernel-rs`.
- [ ] **OCSF-compatible event schema.** Map `ItemInfo` variants to the [OCSF](https://schema.ocsf.io/) classes for direct ingestion into SIEMs that support it.
- [ ] **Per-CPU ring buffer** with `KEVENT`-based signaling so the client can block-wait for new events instead of polling.
- [ ] **ACL on the device object** via `IoCreateDeviceSecure`, so non-admin processes can be denied.
- [ ] **ETW provider** as an alternative transport to the IOCTL/read pipe.

---

## FAQ

**Q: Can I run Valhalla on Linux / macOS?**
No. It is a Windows kernel-mode driver and depends on Windows kernel APIs (`PsSetCreateProcessNotifyRoutineEx`, `CmRegisterCallbackEx`, etc.). The `common` crate will compile on other platforms (it is plain `no_std` Rust), but the driver and client will not.

**Q: Do I need to disable Secure Boot?**
Yes, for test signing. Secure Boot enforces signature requirements that test-signed drivers cannot satisfy. On a VM you can usually leave Secure Boot off.

**Q: Will this work on Windows 7?**
Not as-is. `PsSetCreateProcessNotifyRoutineEx` was extended in Windows Vista+ but is most reliable on Windows 8+. The `windows-kernel-rs` bindings target modern WDK headers. Stick to Windows 10/11.

**Q: Why does the build produce a `.dll` that I rename to `.sys`?**
The cargo `cdylib` target always emits `.dll` on Windows. Kernel drivers must have the `.sys` extension for `sc create` to accept them. `xtask` does the rename automatically.

**Q: How much memory does the driver use?**
The ring is `256 * sizeof(ItemInfo)`. `ItemInfo` is an enum whose largest variant (`ImageLoad`) is roughly 88 bytes, so the ring is ~22 KiB of non-paged pool plus `Vec` bookkeeping. The rest of the driver's footprint is code and readonly data.

**Q: Is this related to Sysinternals Sysmon?**
No. Sysinternals Sysmon is a Microsoft product with a much larger feature set. Valhalla is a small Rust educational driver inspired by the `SysMon` sample in Pavel Yosifovich's *Windows Kernel Programming* book, not by the Sysinternals tool.

**Q: Why is there a `kernel_init` dependency that is only `extern crate`'d?**
The `kernel-init` crate installs a panic handler, an `alloc_error_handler`, and a global allocator suitable for kernel mode. Those are global singletons, so the crate must be linked in even though no symbols are referenced by name. `extern crate kernel_init;` is the explicit "please link this" marker.

**Q: The registry callback only shows HKLM. Can I extend it?**
Yes. Remove or widen the `key_name.contains("\\REGISTRY\\MACHINE")` filter in `OnRegistryNotify`. Be aware that filtering is what keeps the noise level manageable; removing it will produce a flood of events.

---

## Contributing

Contributions are welcome, with the following caveats:

1. **Build must stay green.** `cargo build --release`, `cargo fmt --all --check`, and `cargo clippy --release --lib --bins` must all pass.
2. **Keep `unsafe` confined.** New `unsafe` blocks should be justified in a comment and, where possible, funneled through an existing helper.
3. **Honor the `Cleaner` contract.** Any new kernel resource registered in `DriverEntry` must be paired with a `Cleaner::init_*` so failures roll back cleanly.
4. **No new dependencies without justification.** Kernel-mode dependencies especially must be auditable; the `windows-kernel-rs` family is trusted, anything else needs review.
5. **Commit style.** Use small, focused commits with a `prefix: subject` convention (e.g. `feat: add ETW provider`, `fix: deadlock in registry callback`, `refactor: extract ring buffer`).

Pull requests should target `master`. Force-push history is acceptable on feature branches; do not force-push `master` without coordinating.

---

## Acknowledgments

Valhalla stands on the shoulders of several prior projects and people:

- **[Pavel Yosifovich](https://github.com/zodiacon)** - *Windows Kernel Programming* book and the [SysMon sample](https://github.com/zodiacon/windowskernelprogrammingbook/tree/bd13779bf1f79f4056d206e1f4272baf032e5451/chapter09/SysMon) that this project is a Rust reimagining of.
- **[not-matthias](https://not-matthias.github.io/posts/kernel-driver-with-rust/)** - the excellent *Driver with Rust* blog post that demystified the build pipeline.
- **[radkum/windows-kernel-rs](https://github.com/radkum/windows-kernel-rs)** - the `km-api-sys`, `kernel-string`, `kernel-macros`, `kernel-fast-mutex`, and `kernel-init` crates that make Rust kernel programming ergonomic.
- **[Trantect/winapi-rs](https://github.com/Trantect/winapi-rs)** (feature/km branch) - kernel-mode WDK bindings for Rust.
- **The Rust `no_std` and `embedded` working groups** - whose documentation made the `#![no_std]` + `alloc` story approachable.

---

<p align="center">
  <sub>Built with Rust. Tested on Windows. Broken in kernel mode.</sub>
</p>
