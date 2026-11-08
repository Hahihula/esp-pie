//! ESP32-S31: the `xespv` RISC-V extension (shared with the ESP32-P4).
//!
//! Upstream LLVM doesn't know `xespv`, so the instructions are emitted as raw
//! words, and since their operands are encoded in the words, the asm pins
//! its operands to fixed registers: pointers in a0/a1, result in a2, vector
//! registers q0-q7. The words come from Espressif's GNU assembler
//! (`riscv32-esp-elf-as -march=rv32imafc_xespv`, GCC 15.2 toolchain) for:
//!
//! ```text
//! esp.zero.xacc
//! esp.vld.128.ip q0, a0, 16 / q1, a1, 16 / ... (interleaved as on the S3)
//! esp.vmulas.s8.xacc q0, q1 / q2, q3 / q4, q5 / q6, q7
//! esp.movx.r.xacc.l a2
//! ```

/// CSR 0x7F2 (`CSR_PIE_STATE_REG`) = 1, as ESP-IDF's `rv_utils_enable_pie`.
/// Only core 1 has PIE; on core 0 this does nothing.
#[inline]
pub fn enable() {
    if available() {
        // SAFETY: turning the unit on only allows more instructions.
        unsafe { core::arch::asm!("csrwi 0x7f2, 1", options(nomem, nostack)) };
    }
}

/// True on core 1. ESP-IDF's FreeRTOS port says so too (`portasm.S`: "On
/// the ESP32-S31, the PIE is only available on core 1"); core 0 raises an
/// illegal-instruction exception even with the CSR set.
#[inline(always)]
pub fn available() -> bool {
    let hart: usize;
    // SAFETY: reading mhartid has no side effects.
    unsafe { core::arch::asm!("csrr {0}, mhartid", out(reg) hart, options(nomem, nostack)) };
    hart == 1
}

/// 16 multiply-adds per `esp.vmulas.s8.xacc` into the XACC accumulator.
///
/// # Safety
/// See [`crate::dot_i8x128_unchecked`].
#[inline(always)]
pub unsafe fn dot_i8x128(a: *const i8, b: *const i8) -> i32 {
    let r: i32;
    // SAFETY: forwarded from the caller (alignment, 128 bytes, core 1, PIE on).
    unsafe {
        core::arch::asm!(
            ".4byte 0x0000001b", // esp.zero.xacc
            ".4byte 0x0311009f", // esp.vld.128.ip q0, a0, 16
            ".4byte 0x0311849f", // esp.vld.128.ip q1, a1, 16
            ".4byte 0x0311089f", // esp.vld.128.ip q2, a0, 16
            ".4byte 0x03118c9f", // esp.vld.128.ip q3, a1, 16
            ".4byte 0x06a1a01b", // esp.vmulas.s8.xacc q0, q1
            ".4byte 0x0311109f", // esp.vld.128.ip q4, a0, 16
            ".4byte 0x0311949f", // esp.vld.128.ip q5, a1, 16
            ".4byte 0x4ea1a01b", // esp.vmulas.s8.xacc q2, q3
            ".4byte 0x0311189f", // esp.vld.128.ip q6, a0, 16
            ".4byte 0x03119c9f", // esp.vld.128.ip q7, a1, 16
            ".4byte 0x96a1a01b", // esp.vmulas.s8.xacc q4, q5
            ".4byte 0x0311009f", // esp.vld.128.ip q0, a0, 16
            ".4byte 0x0311849f", // esp.vld.128.ip q1, a1, 16
            ".4byte 0xdea1a01b", // esp.vmulas.s8.xacc q6, q7
            ".4byte 0x0311089f", // esp.vld.128.ip q2, a0, 16
            ".4byte 0x03118c9f", // esp.vld.128.ip q3, a1, 16
            ".4byte 0x06a1a01b", // esp.vmulas.s8.xacc q0, q1
            ".4byte 0x0311109f", // esp.vld.128.ip q4, a0, 16
            ".4byte 0x0311949f", // esp.vld.128.ip q5, a1, 16
            ".4byte 0x4ea1a01b", // esp.vmulas.s8.xacc q2, q3
            ".4byte 0x0311189f", // esp.vld.128.ip q6, a0, 16
            ".4byte 0x03119c9f", // esp.vld.128.ip q7, a1, 16
            ".4byte 0x96a1a01b", // esp.vmulas.s8.xacc q4, q5
            ".4byte 0xdea1a01b", // esp.vmulas.s8.xacc q6, q7
            ".4byte 0x84c0221b", // esp.movx.r.xacc.l a2
            inout("a0") a => _,
            inout("a1") b => _,
            out("a2") r,
            options(nostack, readonly),
        );
    }
    r
}
