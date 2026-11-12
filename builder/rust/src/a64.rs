//! Control-flow classification of AArch64 words, the first step of the
//! lifter. Ported from a64_lift.py's classify().

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    B(u64),
    Bl(u64),
    BCond(u64, u8),
    Cbz(u64, u8, u8, u8),
    Tbz(u64, u8, u8, u8),
    Br(u8),
    Blr(u8),
    Ret(u8),
    Svc(u16),
    Trap,
    Isb,
    Ic,
    Op,
}

pub fn sx(value: u32, bits: u32) -> i64 {
    if value & (1 << (bits - 1)) != 0 {
        value as i64 - (1 << bits)
    } else {
        value as i64
    }
}

pub fn classify(word: u32, pc: u64) -> Kind {
    if word & 0x7C00_0000 == 0x1400_0000 {
        let target = pc.wrapping_add((4 * sx(word & 0x03FF_FFFF, 26)) as u64);
        return if word & 0x8000_0000 != 0 {
            Kind::Bl(target)
        } else {
            Kind::B(target)
        };
    }
    if word & 0xFF00_0010 == 0x5400_0000 {
        return Kind::BCond(
            pc.wrapping_add((4 * sx((word >> 5) & 0x7FFFF, 19)) as u64),
            (word & 15) as u8,
        );
    }
    if word & 0x7E00_0000 == 0x3400_0000 {
        return Kind::Cbz(
            pc.wrapping_add((4 * sx((word >> 5) & 0x7FFFF, 19)) as u64),
            (word & 31) as u8,
            (word >> 31) as u8,
            ((word >> 24) & 1) as u8,
        );
    }
    if word & 0x7E00_0000 == 0x3600_0000 {
        return Kind::Tbz(
            pc.wrapping_add((4 * sx((word >> 5) & 0x3FFF, 14)) as u64),
            (word & 31) as u8,
            (((word >> 31) << 5) | ((word >> 19) & 31)) as u8,
            ((word >> 24) & 1) as u8,
        );
    }
    if word & 0xFFFF_FC1F == 0xD61F_0000 {
        return Kind::Br(((word >> 5) & 31) as u8);
    }
    if word & 0xFFFF_FC1F == 0xD63F_0000 {
        return Kind::Blr(((word >> 5) & 31) as u8);
    }
    if word & 0xFFFF_FC1F == 0xD65F_0000 {
        return Kind::Ret(((word >> 5) & 31) as u8);
    }
    if word & 0xFFE0_001F == 0xD400_0001 {
        return Kind::Svc(((word >> 5) & 0xFFFF) as u16);
    }
    if matches!(word & 0xFFE0_001F, 0xD420_0000 | 0xD440_0000) {
        return Kind::Trap;
    }
    if word & 0xFFFF_F0FF == 0xD503_30DF {
        return Kind::Isb;
    }
    if word & 0xFFFF_FFE0 == 0xD50B_7520 {
        return Kind::Ic;
    }
    Kind::Op
}

pub fn pc_relative(word: u32) -> bool {
    word & 0x1F00_0000 == 0x1000_0000 || word & 0x3B00_0000 == 0x1800_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_targets_are_signed() {
        assert_eq!(classify(0x1400_0002, 0x1000), Kind::B(0x1008));
        assert_eq!(classify(0x97FF_FFFE, 0x1000), Kind::Bl(0xFF8));
        assert_eq!(classify(0x54FF_FFEE, 0x1000), Kind::BCond(0xFFC, 14));
        assert_eq!(classify(0xD65F_03C0, 0), Kind::Ret(30));
        assert_eq!(classify(0xD400_0001, 0), Kind::Svc(0));
        assert_eq!(classify(0x9100_0000, 0), Kind::Op);
    }

    #[test]
    fn pc_relative_forms() {
        assert!(pc_relative(0x1000_0000));
        assert!(pc_relative(0x1800_0000));
        assert!(!pc_relative(0x1400_0000));
    }
}
