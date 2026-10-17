//! FPU registers.

use bitflags::bitflags;

bitflags! {
    pub struct Status: u16 {
        const C3 = 1 << 14;
        const C2 = 1 << 10;
        const C1 = 1 << 9;
        const C0 = 1 << 8;
    }
}

pub struct FPU {
    /// FPU ST0 through ST7 registers.
    pub st: [f64; 8],
    /// Index of top of FPU stack; 8 when stack empty.
    pub st_top: usize,
    /// The result of the last fcom, used to generate the status word.
    /// None is unordered: one of the operands was a NaN.
    pub cmp: Option<std::cmp::Ordering>,
    /// Control word, as managed by fldcw/fnstcw. We only round-trip the value;
    /// precision/rounding control bits are not honored.
    pub control: u16,
}

impl Default for FPU {
    fn default() -> Self {
        Self {
            st: [0.; 8],
            st_top: 8,
            cmp: Some(std::cmp::Ordering::Equal),
            control: 0x037f,
        }
    }
}

impl FPU {
    fn exception(_msg: &str) {
        // TODO: modify state bits etc.
        // At least ignoring these may allow programs to make some progress.
        // See note in https://github.com/joncampbell123/dosbox-x/issues/94 ,
        // "I've seen DOSBox SVN bail out on perfectly good demoscene programs because
        // of [not allowing underflow]."
        // Don't log because anatyda underflows thousands of times, eek.
        // log::warn!("{}", msg);
    }

    /// Get st(0), the current top of the FPU stack.
    pub fn st0(&mut self) -> &mut f64 {
        &mut self.st[self.st_top]
    }

    pub fn push(&mut self, val: f64) {
        if self.st_top == 0 {
            Self::exception("fpu stack overflow");
            return;
        }
        self.st_top -= 1;
        self.st[self.st_top] = val;
    }

    pub fn pop(&mut self) {
        if self.st_top == 8 {
            Self::exception("fpu stack underflow");
            return;
        }
        self.st_top += 1;
    }

    /// Index in self.st for a given ST0, ST1 etc reg.
    fn st_offset(&self, ofs: usize) -> usize {
        let new = self.st_top + ofs;
        if new >= 8 {
            Self::exception("fpu stack underflow");
            return 7;
        }
        new
    }

    pub fn swap(&mut self, o1: usize, o2: usize) {
        let o1 = self.st_offset(o1);
        let o2 = self.st_offset(o2);
        self.st.swap(o1, o2);
    }

    pub fn get(&self, ofs: usize) -> f64 {
        self.st[self.st_offset(ofs)]
    }

    pub fn set(&mut self, ofs: usize, val: f64) {
        self.st[self.st_offset(ofs)] = val;
    }

    pub fn status(&self) -> u16 {
        let status = match self.cmp {
            Some(std::cmp::Ordering::Less) => Status::C0,
            Some(std::cmp::Ordering::Equal) => Status::C3,
            Some(std::cmp::Ordering::Greater) => Status::empty(),
            // Unordered sets all three. After fnstsw/sahf, C2 lands in PF,
            // which is what the jp following a float compare tests for.
            None => Status::C0 | Status::C2 | Status::C3,
        };
        // Our status register impl doesn't include st_top so include it here.
        let mut status = status.bits();
        status |= (self.st_top as u16 & 0b111) << 11;
        status
    }

    pub fn round(&self, val: f64) -> f64 {
        // TODO: rounding modes?
        // This implements default rounding mode of round towards even.
        val.round_ties_even()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Flags;

    fn condition_codes(fpu: &FPU) -> u16 {
        fpu.status() & Status::all().bits()
    }

    #[test]
    fn compare_condition_codes() {
        let mut fpu = FPU::default();
        fpu.cmp = 1.0f64.partial_cmp(&2.0);
        assert_eq!(condition_codes(&fpu), Status::C0.bits());
        fpu.cmp = 2.0f64.partial_cmp(&2.0);
        assert_eq!(condition_codes(&fpu), Status::C3.bits());
        fpu.cmp = 2.0f64.partial_cmp(&1.0);
        assert_eq!(condition_codes(&fpu), 0);
        fpu.cmp = f64::NAN.partial_cmp(&1.0);
        assert_eq!(
            condition_codes(&fpu),
            (Status::C0 | Status::C2 | Status::C3).bits()
        );
    }

    #[test]
    fn unordered_reaches_parity_flag() {
        // fnstsw ax; sahf copies the high byte of the status word into flags.
        let mut fpu = FPU::default();
        fpu.cmp = 1.0f64.partial_cmp(&1.0);
        let ah = (fpu.status() >> 8) as u32;
        assert!(!Flags::from_bits_retain(ah).contains(Flags::PF));
        fpu.cmp = f64::NAN.partial_cmp(&1.0);
        let ah = (fpu.status() >> 8) as u32;
        assert!(Flags::from_bits_retain(ah).contains(Flags::PF));
    }
}

/// Convert an x87 80-bit extended value (little-endian bytes) to f64 (lossy).
pub fn f80_to_f64(b: [u8; 10]) -> f64 {
    let mant = u64::from_le_bytes(b[0..8].try_into().unwrap());
    let se = u16::from_le_bytes([b[8], b[9]]);
    let sign = if se & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (se & 0x7fff) as i32;
    if exp == 0 && mant == 0 {
        return sign * 0.0;
    }
    if exp == 0x7fff {
        return if mant << 1 == 0 { sign * f64::INFINITY } else { f64::NAN };
    }
    sign * (mant as f64) * 2f64.powi(exp - 16383 - 63)
}

/// Convert f64 to an x87 80-bit extended value (little-endian bytes).
pub fn f64_to_f80(v: f64) -> [u8; 10] {
    let bits = v.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (e, m): (u16, u64) = if exp == 0 && frac == 0 {
        (0, 0)
    } else if exp == 0x7ff {
        (0x7fff, (1u64 << 63) | (frac << 11))
    } else if exp == 0 {
        // denormal f64: normalize
        let lz = frac.leading_zeros() as i32; // >= 12
        let m = frac << lz;
        ((1 - 1023 + 16383 - (lz - 11)) as u16, m)
    } else {
        ((exp - 1023 + 16383) as u16, (1u64 << 63) | (frac << 11))
    };
    let mut out = [0u8; 10];
    out[0..8].copy_from_slice(&m.to_le_bytes());
    out[8..10].copy_from_slice(&(sign | e).to_le_bytes());
    out
}
