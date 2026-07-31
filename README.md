# esp-nimble-host

An async `no_std` Bluetooth Low Energy **host** for ESP32 RISC-V chips: the Apache NimBLE host stack wrapped in safe
async Rust for [Embassy](https://embassy.dev/), running against Espressif's BLE controller via `esp-radio`.

The point of this crate is not to be another Rust BLE stack. It is to offer a **credible path to Bluetooth
qualification** for products written in Rust.

## Why this exists

Rust's embedded ecosystem has no qualified BLE host. At the time of writing, [
`trouble`](https://github.com/embassy-rs/trouble) is essentially the only pure-Rust BLE host - it is a good project, but
its feature coverage is still incomplete and, more importantly for commercial work, it is neither qualified nor
pre-qualified with the Bluetooth SIG. If you want to ship a Bluetooth product written in Rust and you need to satisfy a
qualification process, you currently have no obvious route.

This crate takes a different approach: rather than writing a host in Rust, it reuses components that already have a
qualification track record and keeps the Rust contribution as thin and as clearly-bounded as possible.

```
┌─────────────────────────────────────────────────────────┐
│  Your application (Rust, Embassy)                       │
├─────────────────────────────────────────────────────────┤
│  esp-nimble-host         ← this repo: safe async Rust   │
│                            wrappers + build integration │
├─────────────────────────────────────────────────────────┤
│  Apache NimBLE host (C, unmodified protocol logic)      │  ◀── host stack with an
│    GAP · GATT · ATT · L2CAP · SM                        │      established record in
│                                                         │      qualified products
├─────────────────────────────────────────────────────────┤
│  NimBLE Porting Layer (NPL)                             │  ◀── the glue: mapped onto
│    → esp-radio RTOS driver interface (esp-rtos)         │      esp-rtos primitives
│    → ESP ROM functions                                  │      and ROM functions
├─────────────────────────────────────────────────────────┤
│  HCI (H4 framing)                                       │
├─────────────────────────────────────────────────────────┤
│  Espressif BLE controller blob (via esp-radio/esp-hal)  │  ◀── pre-qualified
│                                                         │      controller
└─────────────────────────────────────────────────────────┘
```

The argument, in short:

- The **controller** is Espressif's, used as-is through the `esp-hal` / `esp-radio` ecosystem. Espressif publishes
  qualified designs for its Bluetooth subsystems, so the controller is not something this project needs to re-qualify.
- The **host** is Apache NimBLE, a stack widely used in products that have gone through Bluetooth qualification. Its
  protocol logic is compiled from upstream sources and is not reimplemented here.
- What this project actually adds is the **NPL glue and the Rust API surface**. NPL is a porting layer - timers,
  mutexes, event queues, memory pools - not protocol behaviour. Because the qualifiable protocol logic sits above it
  untouched, adapting NPL to `esp-rtos` should not, in principle, undermine an argument for reusing NimBLE's
  qualification in this configuration.

That is the reasoning. It is a design intended to *keep qualification reachable*, not a claim that qualification has
happened.

## Read this before you rely on any of the above

**This project is not qualified, not certified, and not pre-qualified.** Nothing in this repository has been submitted
to, reviewed by, or blessed by the Bluetooth SIG.

Specifically:

- **The Bluetooth SIG has not been consulted.** The rationale above is our own engineering judgement about why this
  composition *should* be qualifiable. It has not been tested against the SIG's actual process, and the SIG may simply
  disagree.
- **Qualification is per end product.** Reusing pre-qualified components does not make your product qualified. You are
  responsible for the qualification and listing of whatever you ship, including any required testing of the
  host/controller combination.
- **It is on you to verify the component claims.** Which Espressif QDIDs apply to your exact chip, module, and blob
  version - and what NimBLE's qualification status is for the version you build - are facts you must confirm from the
  SIG's listings and from Espressif, not from this README.
- **This build patches NimBLE C sources** (see [Modifications to NimBLE](#modifications-to-nimble)). One patch touches
  the host proper, not just the porting layer. The changes are memory-lifecycle fixes rather than protocol changes, but
  they are modifications to the stack and you should treat them as something to disclose and discuss if you pursue
  qualification.
- **Trademark and membership obligations are yours.** Using Bluetooth technology and branding commercially carries
  Bluetooth SIG membership and licensing requirements independent of this code.

If you are heading toward a commercial launch, talk to the Bluetooth SIG and, ideally, a qualification consultant or an
authorised test lab early. Treat this repository as a technical starting point that was built with qualification in
mind - not as evidence of compliance.

## Status

Working today, exercised on hardware:

- **Scanning / observer** - active and passive discovery, multi-subscriber advertisement stream, raw AD payloads plus
  parsed advertisement fields.
- **Central** - connect, disconnect, connection-event stream (MTU changes, connection-parameter updates, disconnects),
  MTU exchange.
- **GATT client** - service / characteristic / descriptor discovery (all, or by service UUID), attribute read and write
  (with and without response, long writes handled against the negotiated MTU), notification and indication stream.
- **Pairing** - Legacy passkey pairing (`BLE_SM_IOACT_INPUT` / `DISP`). Disabled in the default configuration.

Not implemented:

- **Peripheral / broadcaster roles.** There is no advertising API and no GATT server API. `nimble_sys/gatts.rs` contains
  only groundwork. The NimBLE config defaults reflect this (`peripheral = false`, `broadcaster = false`), though the
  underlying C stack can be compiled with those roles enabled.
- **Extended advertising.** `BLE_EXT_ADV` is off; `BLE_GAP_EVENT_EXT_DISC` is logged and ignored.
- **ISO / LE Audio.** `ble_transport_to_ll_iso_impl` returns `BLE_HS_ENOTSUP`.
- **Secure Connections pairing and bonding** are compiled out by default; the SM options exist in the configuration but
  are not exercised.

Supported chips: **ESP32-C6**, **ESP32-C61**, **ESP32-C5** - all RISC-V. There is no Xtensa support; the build hardcodes
a RISC-V target for the C compilation.

There are no unit or integration tests in this crate; verification happens in consuming applications on real hardware.

## Requirements

- **Rust nightly.** `src/lib.rs` uses `#![feature(c_size_t)]`.
- **A RISC-V bare-metal target**, e.g. `riscv32imac-unknown-none-elf`.
- **`clang`** on the build host, with RISC-V target support. NimBLE is cross-compiled with `clang`, not with GCC.
- **Network access on the first build.** `build.rs` downloads the NimBLE `nimble_1_9_0_tag` tarball into `OUT_DIR`. It
  is cached afterwards, but `cargo clean` forces a re-download.
- **SSH access to `github.com/peeriot/esp-hal`.** `esp-hal` and `esp-radio` are pulled from a private fork (branch
  `feature/ble-host-npl-upstream`) that carries the `ble-host-npl` NPL implementation. This is the piece that makes
  NimBLE run on `esp-rtos`; upstream `esp-radio` does not provide it.

```toml
[dependencies]
esp-nimble-host = { git = "ssh://git@github.com/peeriot/esp-nimble-host.git", features = ["esp32c6"] }
```

Building the library on its own:

```bash
cargo +nightly clippy --target riscv32imac-unknown-none-elf --features esp32c6
cargo +nightly build --release --target riscv32imac-unknown-none-elf --features esp32c6
```

Use `--release` for anything that runs on hardware - `esp-hal` warns about this, and the dev profile is slow enough to
disturb timing-sensitive peripherals.

## Getting started

Three long-running tasks must be up before any BLE API is touched, and **the priorities they run at matter**. The HCI
transport has to outrank the host, which has to outrank the controller; if the transport is starved, controller-to-host
packets back up and leak.

A working arrangement, as used in production:

| Task                                                      | Priority | Kind                                                 |
|-----------------------------------------------------------|----------|------------------------------------------------------|
| HCI transport (`transport_task_rx` + `transport_task_tx`) | 40       | dedicated OS thread running its own Embassy executor |
| NimBLE host (`host_task`)                                 | 30       | OS task                                              |
| BLE controller                                            | 29       | OS task, spawned by `esp-radio`                      |
| Application                                               | 1        | main Embassy executor                                |

`host_task` runs `nimble_port_run()` and **must not be spawned as an Embassy task** - it does not yield to the async
executor. Spawn it as an OS task.

```rust
// Sketch - see a consuming application for a complete, compiling setup.

// 1. Start the RTOS.
esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

// 2. HCI transport on a dedicated high-priority thread with its own executor.
extern "C" fn ble_transport_thread(_: *mut c_void) {
    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    let executor = EXECUTOR.init(Executor::new());
    executor.run(|spawner| {
        let bluetooth = unsafe { BT::steal() };
        let connector = BleConnector::new(bluetooth, Default::default()).unwrap();
        let transport = HostTransport::new(connector); // initialises NimBLE
        spawner.spawn(ble_transport_tx()).unwrap();    // -> transport_task_tx()
        spawner.spawn(ble_transport_rx(transport)).unwrap(); // -> transport_task_rx(t)
    });
}

unsafe {
    esp_radio_rtos_driver::task_create(
        "HCI transport", ble_transport_thread, core::ptr::null_mut(), 40, None, BLE_HCI_STACK,
    );
    // 3. The NimBLE host event loop as an OS task.
    esp_radio_rtos_driver::task_create(
        "BLE Host", esp_nimble_host::host_task, core::ptr::null_mut(), 30, None, BLE_HOST_STACK,
    );
}

// 4. Wait for host/controller sync, then use the API.
esp_nimble_host::wait_for_sync().await;

let mut scanner = Scanner::new();
let mut advs = scanner.subscribe() ?;
scanner.start_scan(None) ?;

while let WaitResult::Message(raw) = advs.next_message().await {
let peripheral = Peripheral::new(raw.addr().clone());
peripheral.connect().await ?;
peripheral.discover_all_services().await ?;
// read / write / subscribe ...
}
```

To receive notifications, subscribe with `Peripheral::subscribe()` **and** enable them on the remote device by writing
the CCCD via `write_descriptor` - there is no combined helper.

## Configuration

`nimble-config.toml` drives the compile-time configuration of the NimBLE stack: roles, maximum connections, HCI
transport buffer counts and sizes, the msys mbuf pool, GATT MTU and procedure limits, L2CAP, bonding storage, and the
security manager. `build.rs` turns it into `MYNEWT_VAL_*` C defines that override NimBLE's `syscfg.h` defaults.

The file is extensively commented, including RAM and flash cost estimates per option - read it before changing sizing.
Defaults target a central + observer workload with security disabled.

To use your own configuration from a consuming project, either place `nimble-config.toml` at your Cargo workspace root
(found automatically via `CARGO_WORKSPACE_DIR`) or set `NIMBLE_CONFIG_DIR` in your `.cargo/config.toml`. Missing values
fall back to built-in defaults.

Changing this file changes what gets compiled: enabling `security.legacy` or `security.sc` additionally pulls in
`ext/tinycrypt`, and disabling roles compiles that code out entirely.

## Modifications to NimBLE

NimBLE is downloaded at build time rather than vendored, and `build.rs` applies two source patches before compiling.
Both are memory-lifecycle fixes required to run NimBLE against the `esp-radio` NPL; neither changes protocol behaviour.
Both are applied by string match and **deliberately panic if the expected pattern is missing**, so a NimBLE version bump
fails loudly instead of silently dropping a fix.

- **`porting/nimble/src/os_mempool.c`** - zero each memory-pool block on allocation. NimBLE threads its free-list
  pointer through the first bytes of a freed block; the NPL stores a heap `Event` pointer in `ble_npl_event.dummy`, and
  `ble_npl_event_init` skips initialisation when `dummy != 0`. Without this patch a recycled block looks
  already-initialised and the stale free-list pointer is eventually called as a function pointer, producing an Illegal
  Instruction crash.
- **`nimble/host/src/ble_hs.c`** - call `ble_npl_event_deinit()` before returning an event block to
  `ble_hs_hci_ev_pool`, otherwise the heap `Event` leaks on every recycle. As of NimBLE 1.9 this is the only such call
  site.

The second patch modifies the host, not the porting layer. It is small and non-behavioural, but if you pursue
qualification, treat both patches as material to disclose. See the doc comments in `build.rs` for the full rationale.

## Repository layout

| Path                                                          | Contents                                                                           |
|---------------------------------------------------------------|------------------------------------------------------------------------------------|
| `src/lib.rs`                                                  | Scanner, `HostTransport`, HCI transport tasks, host sync, C FFI callbacks          |
| `src/peripheral.rs`                                           | `Peripheral` - connect, pair, GATT operations, GAP event dispatch                  |
| `src/discovery.rs`, `src/characteristic.rs`, `src/service.rs` | GATT client discovery and attribute access                                         |
| `src/data.rs`, `src/error.rs`                                 | addresses, advertisements, conversions, error taxonomy                             |
| `src/nimble_sys/`                                             | the FFI boundary - safe wrappers over the generated bindings                       |
| `src/libc.rs`                                                 | libc shims NimBLE links against (the rest come from `tinyrlibc`)                   |
| `nimble/`                                                     | freestanding libc header stubs that shadow system headers during the cross-compile |
| `build.rs`                                                    | config generation, NimBLE download, patching, bindgen, C compilation               |
| `nimble-config.toml`                                          | compile-time NimBLE stack configuration                                            |
| `prj/project.yml`                                             | Mynewt `newt` descriptor, used only to regenerate syscfg reference values          |
