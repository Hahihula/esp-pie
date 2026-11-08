# esp-pie

`no_std` Rust SIMD kernels for the PIE (Processor Instruction Extensions)
vector units of Espressif chips:

| chip | feature | PIE on | how |
|---|---|---|---|
| ESP32-S3 | `esp32s3` | both cores | Xtensa `ee.*` inline asm (needs the `esp` toolchain) |
| ESP32-S31 | `esp32s31` | core 1 only | RISC-V `xespv` as raw instruction words (stable Rust) |
| anything else | none | — | plain Rust, same results |

Without a chip feature every kernel is the portable fallback, so a library
built on esp-pie also compiles and tests on a PC. All kernels are integer
math and give bit-identical results to the fallback.

Today the crate has what [cactus-whistle-rs](https://github.com/Hahihula/cactus-whistle-rs)
needs: a 128-lane int8 dot product. More kernels will follow as other models
need them.

## Use

The crate isn't on crates.io; depend on it by git tag (once the repository has
one; `rev = "..."` until then), with the chip feature:

```toml
[dependencies]
esp-pie = { git = "https://github.com/Hahihula/esp-pie", tag = "v0.1.0", features = ["esp32s3"] }
```

```rust
use esp_pie::{I8x128, Pie, SelfTest};

// Once, on the core that runs the kernels (on the ESP32-S31: core 1).
let pie = Pie::enable(); // None without PIE on this core
assert_ne!(esp_pie::self_test(), SelfTest::Failed);

let a = I8x128([1; 128]);
let b = I8x128([-2; 128]);
let dot = match pie {
    Some(pie) => pie.dot_i8x128(&a, &b), // no per-call core check
    None => esp_pie::dot_i8x128(&a, &b),
};
assert_eq!(dot, -256);
```

A chip feature on a PC builds with the fallback (so libraries using esp-pie
test on a PC). On the wrong chip of the same architecture it is a compile
error: `esp32s3` needs `xtensa-esp32s3-none-elf`, `esp32s31` needs
`riscv32imafc-unknown-none-elf` (not the ESP32-C3/C6/H2's `riscv32imac`).

| item | what it does |
|---|---|
| `enable()` | switches PIE on for the calling core (S3: `CPENABLE` bit 3; S31: CSR `0x7F2`) |
| `available()` | whether the calling core has PIE (S31: reads `mhartid`, true on core 1) |
| `Pie::enable()` | `enable()`, then a core-bound token if this core has PIE; its kernels skip the per-call check |
| `self_test()` | runs every entry point against the fallback; returns `SelfTest::Pie`, `FallbackOnly` (no PIE on this core: nothing checked) or `Failed` |
| `I8x128` | 128 int8 lanes, 16-byte aligned; `I8x128::from_ref(&[i8; 128])` views an aligned array as one |
| `dot_i8x128(&a, &b)` | dot product; plain Rust on a core without PIE |
| `dot_i8x128_blocks(&[I8x128], &[I8x128])` | over several blocks, exact (`i64`); `None` on a length mismatch |
| `unsafe dot_i8x128_unchecked(ptr, ptr)` | always the PIE path with no core check, for engines that check `available()` once per batch; alignment is only asserted in debug builds (see below) |
| `dot_i8x128_scalar(&[i8; 128], &[i8; 128])` | the fallback, any alignment |

## Rules

- **Enable per core.** Without `enable()` the first vector instruction raises
  an exception (S3: coprocessor disabled; S31: illegal instruction).
- **Aligned operands.** The safe kernels take `I8x128`, which is aligned.
  With `dot_i8x128_unchecked`, a misaligned pointer is the caller's bug, and
  release builds don't catch it: the ESP32-S3's vector loads ignore the low
  address bits and silently read the aligned-down block (a wrong result, no
  fault); the ESP32-S31 handles misaligned loads in hardware.
- **One PIE task per core under esp-rtos.** esp-rtos (the Rust RTOS) doesn't
  save the vector registers and accumulators on a context switch, so run the
  kernels from one task per core, or give them a core of their own. (ESP-IDF's
  FreeRTOS is different: it switches PIE context lazily and pins a task to its
  core once the task uses PIE.)
- **ESP32-S31: core 1 only.** On bare metal and under esp-rtos the first
  vector instruction on core 0 raises an illegal-instruction exception, even
  with the CSR set. (ESP-IDF's FreeRTOS catches that trap and migrates the task
  to core 1, `vectors.S`; nothing does that here.) `available()` and `Pie`
  tell the cores apart. `available()` is a CSR read on the S31, so in hot
  loops hold a `Pie` or ask once per batch of work (asking per call cost
  Whistle 3% of its encode time).
- **SAR.** The dot product uses `ee.vmulas.s8.accx`, which accumulates full
  products and doesn't read SAR, so it needs no SAR setup on the S3.

## Speed

Whistle speech recognition on the ESP32-S31-Korvo-1, one core, the same
transcription (C-engine spec):

| | plain Rust | esp-pie |
|---|---|---|
| encode | 23.9 s | 6.6 s |
| decode | 17.7 s | 8.2 s |
| total | 41.6 s | 14.8 s |

## How the S31 words were made

Upstream LLVM doesn't know `xespv`, so the S31 kernels are `.4byte` words
with their operands pinned to fixed registers (pointers a0/a1, result a2,
vector registers q0-q7). The words come from Espressif's GNU assembler
(`riscv32-esp-elf-as -march=rv32imafc_xespv`, GCC 15.2 toolchain); each word
has the instruction as a comment in `src/esp32s31.rs`.

## Development

```bash
cargo test                                         # fallback and doctests, on the PC
rustup target add riscv32imafc-unknown-none-elf    # once
cargo +stable build --release --target riscv32imafc-unknown-none-elf --features esp32s31
cargo +esp build --release -Zbuild-std=core --target xtensa-esp32s3-none-elf --features esp32s3
```

`rust-version` (1.85) covers the fallback and the ESP32-S31. The ESP32-S3 path
needs the `esp` toolchain (`espup install`): inline assembly on Xtensa is
still behind a nightly feature gate there.

GitHub Actions (`.github/workflows/ci.yml`) runs formatting, clippy, the tests
and doctests, and builds for both chips, including checks that the chip
features refuse the wrong chips.

The on-chip check is `self_test()`; the Whistle firmwares run it at boot and
expect `SelfTest::Pie`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
