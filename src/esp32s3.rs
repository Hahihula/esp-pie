//! ESP32-S3: Xtensa `ee.*` instructions (the `esp` toolchain's LLVM
//! assembles them).

/// Coprocessor 3 ("AI" in the S3 core config) next to the FPU (CP0).
/// `CPENABLE` is per core and saved around interrupts.
#[inline]
pub fn enable() {
    // SAFETY: setting a CPENABLE bit only allows more instructions.
    unsafe {
        let mut v: u32;
        core::arch::asm!("rsr {0}, cpenable", out(reg) v, options(nomem, nostack));
        v |= 1 << 3;
        core::arch::asm!("wsr {0}, cpenable", "rsync", in(reg) v, options(nomem, nostack));
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
            options(nostack, readonly),
        );
    }
    r
}
