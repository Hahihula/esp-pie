//! SIMD kernels for the PIE (Processor Instruction Extensions) vector units
//! of Espressif chips, for `no_std` firmware.
//!
//! | chip | feature | PIE | notes |
//! |---|---|---|---|
//! | ESP32-S3 | `esp32s3` | both cores | Xtensa `ee.*` instructions, `esp` toolchain |
//! | ESP32-S31 | `esp32s31` | core 1 only | RISC-V `xespv`, stable Rust |
//! | ESP32-P4 (rev v3.0+) | `esp32p4` | both HP cores | RISC-V `xespv` (same words as the S31), stable Rust |
//! | anything else | none | — | portable Rust fallback, same results |
//!
//! A chip feature on a target of another architecture (e.g. a PC) also
//! builds, with the fallback, so code using the crate can be tested on a
//! PC. A chip feature on the wrong chip of the right architecture (another
//! Xtensa chip, or a RISC-V target without the F extension such as the
//! ESP32-C3/C6) is a compile error. The ESP32-S31 and the ESP32-P4 share a
//! target (`riscv32imafc-unknown-none-elf`), so between those two only the
//! feature tells them apart.
//!
//! ```
//! use esp_pie::{I8x128, Pie, SelfTest};
//!
//! // Once, on the core that runs the kernels (on the ESP32-S31: core 1).
//! let pie = Pie::enable(); // None without PIE on this core
//! assert_ne!(esp_pie::self_test(), SelfTest::Failed);
//!
//! let a = I8x128([1; 128]);
//! let b = I8x128([-2; 128]);
//! let dot = match pie {
//!     Some(pie) => pie.dot_i8x128(&a, &b),
//!     None => esp_pie::dot_i8x128(&a, &b),
//! };
//! assert_eq!(dot, -256);
//! ```
//!
//! # Rules
//!
//! - Enable PIE on each core that runs PIE code ([`enable`] or
//!   [`Pie::enable`]). Without it the first vector instruction raises an
//!   exception (S3: coprocessor disabled, S31/P4: illegal instruction).
//! - Under esp-rtos (the Rust RTOS) only one task per core may use PIE: it
//!   does not save the vector registers (q0-q7) and accumulators on a context
//!   switch. Run the kernels from one task, or from a core of their own.
//!   (ESP-IDF's FreeRTOS switches PIE context lazily instead, and pins a
//!   task to its core once it uses PIE; that doesn't apply here.)
//! - On the ESP32-S31 only core 1 has PIE. [`available`] and [`Pie`] tell the
//!   cores apart, and the safe kernels fall back to plain Rust on core 0.
//!   The ESP32-P4 has PIE on both HP cores.
//! - ESP32-P4: chip revision v3.0 or later. Earlier revisions implement an
//!   older PIE encoding (`xespv2p1`); [`self_test`] fails there.
//!
//! Every kernel gives bit-identical results to its fallback (integer math).

#![no_std]
#![cfg_attr(
    all(target_arch = "xtensa", feature = "esp32s3"),
    feature(asm_experimental_arch)
)]

#[cfg(any(
    all(feature = "esp32s3", feature = "esp32s31"),
    all(feature = "esp32s3", feature = "esp32p4"),
    all(feature = "esp32s31", feature = "esp32p4"),
))]
compile_error!("esp-pie: enable only one chip feature (`esp32s3`, `esp32s31` or `esp32p4`)");

#[cfg(all(
    feature = "esp32s3",
    target_arch = "xtensa",
    not(target_feature = "esp32s3ops")
))]
compile_error!(
    "esp-pie: the `esp32s3` feature needs the ESP32-S3 target (xtensa-esp32s3-none-elf); \
     this Xtensa chip has no PIE"
);

// Stable Rust doesn't expose the F extension as a cfg; `zicsr` comes with it
// and distinguishes riscv32imafc (ESP32-S31, ESP32-P4) from riscv32imac
// (C3, C6, H2, and the P4's LP core).
#[cfg(all(
    feature = "esp32s31",
    target_arch = "riscv32",
    not(target_feature = "zicsr")
))]
compile_error!(
    "esp-pie: the `esp32s31` feature needs the ESP32-S31 target (riscv32imafc-unknown-none-elf); \
     this RISC-V chip has no PIE"
);
#[cfg(all(
    feature = "esp32p4",
    target_arch = "riscv32",
    not(target_feature = "zicsr")
))]
compile_error!(
    "esp-pie: the `esp32p4` feature needs the ESP32-P4 HP-core target (riscv32imafc-unknown-none-elf); \
     this RISC-V core has no PIE"
);

use core::marker::PhantomData;

#[cfg(all(target_arch = "xtensa", feature = "esp32s3"))]
mod esp32s3;
#[cfg(all(target_arch = "xtensa", feature = "esp32s3"))]
use esp32s3 as chip;

#[cfg(all(
    target_arch = "riscv32",
    any(feature = "esp32s31", feature = "esp32p4")
))]
mod xespv;
#[cfg(all(
    target_arch = "riscv32",
    any(feature = "esp32s31", feature = "esp32p4")
))]
use xespv as chip;

#[cfg(not(any(
    all(target_arch = "xtensa", feature = "esp32s3"),
    all(
        target_arch = "riscv32",
        any(feature = "esp32s31", feature = "esp32p4")
    )
)))]
mod chip {
    pub fn enable() {}

    pub fn available() -> bool {
        false
    }

    /// # Safety
    /// 128 readable bytes behind each pointer.
    #[inline(always)]
    pub unsafe fn dot_i8x128(a: *const i8, b: *const i8) -> i32 {
        // SAFETY: the caller guarantees 128 readable bytes behind each pointer.
        unsafe { super::dot_i8x128_scalar(&*a.cast::<[i8; 128]>(), &*b.cast::<[i8; 128]>()) }
    }
}

/// 128 int8 lanes, 16-byte aligned as the PIE loads require.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(16))]
pub struct I8x128(
    /// The lanes.
    pub [i8; 128],
);

impl I8x128 {
    /// All lanes zero.
    pub const ZERO: Self = Self([0; 128]);

    /// `lanes` viewed as an [`I8x128`], if it is 16-byte aligned. A cheap
    /// way to use the safe kernels on an engine's own buffers.
    #[must_use]
    pub fn from_ref(lanes: &[i8; 128]) -> Option<&Self> {
        let ptr = lanes.as_ptr().cast::<Self>();
        // SAFETY: I8x128 is repr(C) around [i8; 128]; only the alignment
        // differs, and it is checked first.
        ptr.is_aligned().then(|| unsafe { &*ptr })
    }
}

impl Default for I8x128 {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<[i8; 128]> for I8x128 {
    fn from(lanes: [i8; 128]) -> Self {
        Self(lanes)
    }
}

impl AsRef<[i8; 128]> for I8x128 {
    fn as_ref(&self) -> &[i8; 128] {
        &self.0
    }
}

impl AsMut<[i8; 128]> for I8x128 {
    fn as_mut(&mut self) -> &mut [i8; 128] {
        &mut self.0
    }
}

/// Enables PIE on the calling core: ESP32-S3 sets coprocessor 3 in
/// `CPENABLE`, ESP32-S31 and ESP32-P4 write CSR `0x7F2` (ESP-IDF
/// `rv_utils_enable_pie`).
/// On S31 core 0, and without a chip feature, it does nothing.
#[inline]
pub fn enable() {
    chip::enable();
}

/// Whether the calling core has PIE (it still needs [`enable`]): always on
/// the ESP32-S3 and the ESP32-P4, core 1 only on the ESP32-S31, never without
/// a chip feature.
///
/// On the ESP32-S31 this reads `mhartid`. In a hot loop, hold a [`Pie`] (or
/// ask once) rather than checking on every kernel call.
#[inline]
#[must_use]
pub fn available() -> bool {
    chip::available()
}

/// Proof that PIE is available and enabled on the current core: its kernels
/// run PIE without re-checking which core they are on. Not `Send`, since it
/// belongs to the core that created it.
#[derive(Clone, Copy, Debug)]
pub struct Pie {
    _core_bound: PhantomData<*const ()>,
}

impl Pie {
    /// Enables PIE on the calling core and returns the token, or `None` if
    /// this core has no PIE (ESP32-S31 core 0, or no chip feature).
    #[must_use]
    pub fn enable() -> Option<Self> {
        enable();
        Self::here()
    }

    /// The token if this core has PIE, without enabling it: for code that
    /// runs after [`enable`] on this core.
    fn here() -> Option<Self> {
        available().then_some(Self {
            _core_bound: PhantomData,
        })
    }

    /// [`dot_i8x128`] on PIE, without the per-call core check.
    #[inline]
    #[must_use]
    pub fn dot_i8x128(self, a: &I8x128, b: &I8x128) -> i32 {
        // SAFETY: I8x128 is 128 bytes, 16-byte aligned; the token proves PIE
        // is available and enabled on this core.
        unsafe { chip::dot_i8x128(a.0.as_ptr(), b.0.as_ptr()) }
    }

    /// [`dot_i8x128_blocks`] on PIE, without the per-call core check.
    #[must_use]
    pub fn dot_i8x128_blocks(self, a: &[I8x128], b: &[I8x128]) -> Option<i64> {
        (a.len() == b.len()).then(|| {
            a.iter()
                .zip(b)
                .map(|(x, y)| i64::from(self.dot_i8x128(x, y)))
                .sum()
        })
    }
}

/// `sum(a[i] * b[i])` over 128 int8 lanes, with PIE when the calling core
/// has it. |result| <= 128 * 128 * 128 = 2^21, so it never overflows.
///
/// Checks the core on every call; on the ESP32-S31 that is a CSR read
/// (`mhartid`), which cost Whistle 3% of its encode time when done per dot.
/// In loops, use [`Pie::dot_i8x128`] with a token from [`Pie::enable`].
#[inline]
#[must_use]
pub fn dot_i8x128(a: &I8x128, b: &I8x128) -> i32 {
    match Pie::here() {
        Some(pie) => pie.dot_i8x128(a, b),
        None => dot_i8x128_scalar(&a.0, &b.0),
    }
}

/// [`dot_i8x128`] over raw pointers that always takes the PIE path (the
/// fallback without a chip feature), with no core check: for engines that
/// keep their own aligned buffers and check [`available`] once per batch.
///
/// # Safety
///
/// - `a` and `b` point to 128 readable bytes each.
/// - Both are 16-byte aligned. Debug builds assert it; release builds don't
///   check, and then the outcome depends on the chip: the ESP32-S3's vector
///   loads ignore the low address bits and silently read the aligned-down
///   block (a wrong result, no fault), while the ESP32-S31 and ESP32-P4
///   handle misaligned loads in hardware. [`I8x128::from_ref`] checks alignment.
/// - With a chip feature: the calling core has PIE ([`available`]) and it
///   is enabled ([`enable`]).
#[inline(always)] // engines' hot path: keep the kernel inside their loops
#[must_use]
pub unsafe fn dot_i8x128_unchecked(a: *const i8, b: *const i8) -> i32 {
    debug_assert!(
        a.cast::<I8x128>().is_aligned() && b.cast::<I8x128>().is_aligned(),
        "esp-pie: unaligned operand"
    );
    // SAFETY: forwarded from the caller.
    unsafe { chip::dot_i8x128(a, b) }
}

/// The portable version of [`dot_i8x128`], for any alignment. Fixed-size
/// arrays let LLVM drop the bounds checks and vectorise where it can.
#[inline]
#[must_use]
pub fn dot_i8x128_scalar(a: &[i8; 128], b: &[i8; 128]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| i32::from(x) * i32::from(y))
        .sum()
}

/// `sum(a[i] * b[i])` over whole blocks of 128 lanes, with PIE when the
/// calling core has it. Accumulated in `i64`, so it is exact for any length.
/// `None` if `a` and `b` have different lengths.
#[must_use]
pub fn dot_i8x128_blocks(a: &[I8x128], b: &[I8x128]) -> Option<i64> {
    match Pie::here() {
        Some(pie) => pie.dot_i8x128_blocks(a, b),
        None => (a.len() == b.len()).then(|| {
            a.iter()
                .zip(b)
                .map(|(x, y)| i64::from(dot_i8x128_scalar(&x.0, &y.0)))
                .sum()
        }),
    }
}

/// What [`self_test`] could check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfTest {
    /// Every entry point ran on PIE and matched the fallback.
    Pie,
    /// No PIE on this core (or no chip feature): only the fallback could
    /// run, so nothing about PIE was checked.
    FallbackOnly,
    /// A PIE result differed from the fallback.
    Failed,
}

/// Runs every entry point ([`dot_i8x128`], [`Pie`], [`dot_i8x128_unchecked`],
/// the block versions) on pseudo-random data and on the extreme values, and
/// compares with the fallback. Call after [`enable`] on the core that will
/// use PIE: anything but [`SelfTest::Pie`] there means PIE isn't in use or is
/// wrong.
#[must_use]
pub fn self_test() -> SelfTest {
    let Some(pie) = Pie::here() else {
        return SelfTest::FallbackOnly;
    };
    let mut a = [I8x128::ZERO; 2];
    let mut b = [I8x128::ZERO; 2];
    let mut seed = 0x1234_5678u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 24) as i8
    };
    for i in 0..128 {
        a[0].0[i] = next();
        b[0].0[i] = next();
    }
    // Worst case for the accumulator: -128 * -128 in every lane.
    a[1] = I8x128([i8::MIN; 128]);
    b[1] = I8x128([i8::MIN; 128]);
    let want = [0, 1].map(|k| dot_i8x128_scalar(&a[k].0, &b[k].0));
    let blocks = Some(i64::from(want[0]) + i64::from(want[1]));

    let each = (0..2).all(|k| {
        // SAFETY: I8x128 is 128 bytes, 16-byte aligned; `pie` proves the
        // core has PIE, and the caller enabled it.
        let unchecked = unsafe { dot_i8x128_unchecked(a[k].0.as_ptr(), b[k].0.as_ptr()) };
        [
            dot_i8x128(&a[k], &b[k]),
            pie.dot_i8x128(&a[k], &b[k]),
            unchecked,
        ] == [want[k]; 3]
    });
    if each && pie.dot_i8x128_blocks(&a, &b) == blocks && dot_i8x128_blocks(&a, &b) == blocks {
        SelfTest::Pie
    } else {
        SelfTest::Failed
    }
}

/// The README's examples, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (I8x128, I8x128) {
        let mut a = I8x128::ZERO;
        let mut b = I8x128::ZERO;
        for i in 0..128 {
            a.0[i] = (i as i32 - 64) as i8;
            b.0[i] = (3 * i as i32 - 100).clamp(-128, 127) as i8;
        }
        (a, b)
    }

    #[test]
    fn fallback_matches_definition() {
        let (a, b) = pair();
        let want: i32 = (0..128)
            .map(|i| i32::from(a.0[i]) * i32::from(b.0[i]))
            .sum();
        assert_eq!(dot_i8x128(&a, &b), want);
        assert_eq!(
            unsafe { dot_i8x128_unchecked(a.0.as_ptr(), b.0.as_ptr()) },
            want
        );
        assert_eq!(
            dot_i8x128_blocks(&[a, a], &[b, b]),
            Some(2 * i64::from(want))
        );
    }

    #[test]
    fn extremes() {
        let a = I8x128([i8::MIN; 128]);
        assert_eq!(dot_i8x128(&a, &a), 1 << 21);
        let b = I8x128([i8::MAX; 128]);
        assert_eq!(dot_i8x128(&a, &b), -128 * 127 * 128);
    }

    #[test]
    fn blocks_are_exact_past_i32() {
        // 1024 worst-case blocks sum to 2^31, one more than i32 holds.
        let a = [I8x128([i8::MIN; 128]); 1024];
        assert_eq!(dot_i8x128_blocks(&a, &a), Some(1 << 31));
    }

    #[test]
    fn blocks_length_mismatch_is_none() {
        let (a, b) = pair();
        assert_eq!(dot_i8x128_blocks(&[a, a], &[b]), None);
    }

    #[test]
    fn self_test_says_when_only_the_fallback_ran() {
        // No chip feature on the host: nothing about PIE can be checked.
        assert_eq!(self_test(), SelfTest::FallbackOnly);
        assert!(Pie::enable().is_none());
    }

    #[test]
    fn from_ref_checks_alignment() {
        let block = I8x128([7; 128]);
        assert!(I8x128::from_ref(&block.0).is_some());
        #[repr(C, align(16))]
        struct Buf([i8; 129]);
        let buf = Buf([0; 129]);
        let misaligned: &[i8; 128] = buf.0[1..].try_into().unwrap();
        assert!(I8x128::from_ref(misaligned).is_none());
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::align_of::<I8x128>(), 16);
        assert_eq!(core::mem::size_of::<I8x128>(), 128);
    }
}
