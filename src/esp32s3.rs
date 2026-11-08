//! ESP32-S3: Xtensa `ee.*` instructions (the `esp` toolchain's LLVM
//! assembles them).
//!
//! The kernels use the vector registers q0-q7 and the ACCX accumulator
//! without declaring them to the compiler: `rustc` has no register names for
//! them on Xtensa (`out("q0") _` is rejected as an unknown register), and
//! LLVM never allocates them for compiled Rust code. Checked on a whole
//! firmware: every `ee.vld.128.ip`, `ee.vmulas.s8.accx` and `ee.zero.accx`
//! in the binary belongs to an inlined copy of these kernels. Each kernel
//! zeroes or loads everything it reads, so nothing carries over between
//! blocks either.

/// Coprocessor 3 ("AI" in the S3 core config) next to the FPU (CP0).
/// `CPENABLE` is per core. The read-modify-write runs with interrupts
/// masked (`rsil 15`), so a handler that changes `CPENABLE` in between can't
/// have its change overwritten.
#[inline]
pub fn enable() {
    // SAFETY: masks interrupts for four instructions and restores PS;
    // setting a CPENABLE bit only allows more instructions.
    unsafe {
        core::arch::asm!(
            "rsil {ps}, 15",
            "rsr {v}, cpenable",
            "or {v}, {v}, {bit}",
            "wsr {v}, cpenable",
            "rsync",
            "wsr {ps}, ps",
            "rsync",
            ps = out(reg) _,
            v = out(reg) _,
            bit = in(reg) 1u32 << 3,
            options(nomem, nostack),
        );
    }
}

#[inline(always)]
pub fn available() -> bool {
    true
}

/// 16 multiply-adds per `ee.vmulas.s8.accx` into the 40-bit ACCX
/// accumulator; loads and MACs interleaved over q0-q7.
///
/// SAR: the `ee.vmul.*` instructions shift their products right by SAR, so
/// they depend on whatever SAR holds. `ee.vmulas.*.accx` accumulates the
/// full products into ACCX without a shift and doesn't read SAR, so this
/// kernel needs no SAR setup (and the self-test, run against plain integer
/// math, would catch it if it did). Kernels added later that use
/// `ee.vmul.*` must set SAR themselves.
///
/// The S3's ACCX is 40 bits wide and `rur.accx_0` reads its low 32; the
/// S31's XACC differs in width, which matters for kernels that accumulate
/// more than one 128-lane block.
///
/// # Safety
/// See [`crate::dot_i8x128_unchecked`].
#[inline(always)]
pub unsafe fn dot_i8x128(a: *const i8, b: *const i8) -> i32 {
    let r: i32;
    // SAFETY: forwarded from the caller (alignment, 128 bytes, PIE enabled).
    unsafe {
        core::arch::asm!(
            "ee.zero.accx",
            "ee.vld.128.ip q0, {a}, 16", "ee.vld.128.ip q1, {b}, 16",
            "ee.vld.128.ip q2, {a}, 16", "ee.vld.128.ip q3, {b}, 16",
            "ee.vmulas.s8.accx q0, q1",
            "ee.vld.128.ip q4, {a}, 16", "ee.vld.128.ip q5, {b}, 16",
            "ee.vmulas.s8.accx q2, q3",
            "ee.vld.128.ip q6, {a}, 16", "ee.vld.128.ip q7, {b}, 16",
            "ee.vmulas.s8.accx q4, q5",
            "ee.vld.128.ip q0, {a}, 16", "ee.vld.128.ip q1, {b}, 16",
            "ee.vmulas.s8.accx q6, q7",
            "ee.vld.128.ip q2, {a}, 16", "ee.vld.128.ip q3, {b}, 16",
            "ee.vmulas.s8.accx q0, q1",
            "ee.vld.128.ip q4, {a}, 16", "ee.vld.128.ip q5, {b}, 16",
            "ee.vmulas.s8.accx q2, q3",
            "ee.vld.128.ip q6, {a}, 16", "ee.vld.128.ip q7, {b}, 16",
            "ee.vmulas.s8.accx q4, q5",
            "ee.vmulas.s8.accx q6, q7",
            "rur.accx_0 {r}",
            a = inout(reg) a => _,
            b = inout(reg) b => _,
            r = out(reg) r,
            // `readonly`: the asm writes no memory but may read any (so
            // the compiler finishes earlier stores to the operands first).
            options(nostack, readonly),
        );
    }
    r
}
