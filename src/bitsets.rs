use std::num::Wrapping;
use basetypes::*;

#[inline(always)]
pub fn ls1b(x: u64) -> u64 {
    (x as i64 & (Wrapping(0) - Wrapping(x as i64)).0) as u64
}

#[inline(always)]
pub fn reset_ls1b(x: &mut u64) {
    *x &= (Wrapping(*x) - Wrapping(1)).0;
}

#[inline(always)]
pub fn above_ls1b_mask(x: u64) -> u64 {
    assert!(x != 0);
    (x as i64 ^ (Wrapping(0) - Wrapping(x as i64)).0) as u64
}

#[inline(always)]
pub fn below_lsb1_mask_including(x: u64) -> u64 {
    assert!(x != 0);
    x ^ (x - 1)
}

#[inline(always)]
pub fn below_lsb1_mask(x: u64) -> u64 {
    assert!(x != 0);
    !x & (x - 1)
}

#[inline(always)]
pub fn smeared_ls1b_up(x: u64) -> u64 {
    assert!(x != 0);
    ((x as i64) | (Wrapping(0) - Wrapping(x as i64)).0) as u64
}

#[inline(always)]
pub fn smeared_ls1b_down(x: u64) -> u64 {
    assert!(x != 0);
    x | (x - 1)
}

#[inline(always)]
pub fn gen_shift(x: u64, s: isize) -> u64 {
    if s > 0 {
        x << s
    } else {
        x >> -s
    }
}

#[inline(always)]
pub fn bitscan_forward(b: u64) -> Square {
    assert!(b != 0);
    bitscan_1bit(ls1b(b))
}

#[inline(always)]
pub fn bitscan_forward_and_reset(b: &mut u64) -> Square {
    assert!(*b != 0);
    let ls1b = ls1b(*b);
    *b ^= ls1b;
    bitscan_1bit(ls1b)
}

#[inline(always)]
pub fn bitscan_1bit(b: u64) -> Square {
    assert!(b != 0);
    assert_eq!(b, ls1b(b));
    const DEBRUIJN64: Wrapping<u64> = Wrapping(0x03f79d71b4cb0a89);
    const INDEX64: [Square; 64] = [0, 1, 48, 2, 57, 49, 28, 3, 61, 58, 50, 42, 38, 29, 17, 4, 62,
                                   55, 59, 36, 53, 51, 43, 22, 45, 39, 33, 30, 24, 18, 12, 5, 63,
                                   47, 56, 27, 60, 41, 37, 16, 54, 35, 52, 21, 44, 32, 23, 11, 46,
                                   26, 40, 15, 34, 20, 31, 10, 25, 14, 19, 9, 13, 8, 7, 6];
    unsafe { *INDEX64.get_unchecked(((Wrapping(b) * DEBRUIJN64).0 >> 58) as usize) }
}

#[inline]
pub fn pop_count(mut b: u64) -> usize {
   let mut count = 0;
   while b != 0 {
       count += 1;
       reset_ls1b(&mut b);
   }
   count
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ls1b_functions() {
        assert_eq!(ls1b(0x100100), 0x100);
        assert_eq!(ls1b(0x8000000000000000), 0x8000000000000000);
        assert_eq!(ls1b(0xf800000000000000), 0x0800000000000000);
        assert_eq!(ls1b(0), 0);
        let mut x = 0x100100u64;
        reset_ls1b(&mut x);
        assert_eq!(x, 0x100000);
        reset_ls1b(&mut x);
        assert_eq!(x, 0);
        reset_ls1b(&mut x);
        assert_eq!(x, 0);
        assert_eq!(above_ls1b_mask(0b11101000), 0xfffffffffffffff0);
        assert_eq!(above_ls1b_mask(0x8000000000000000), 0);
        assert_eq!(below_lsb1_mask_including(0b11101000), 0b1111);
        assert_eq!(below_lsb1_mask(0b11101000), 0b111);
        assert_eq!(smeared_ls1b_up(0b1010000), 0xfffffffffffffff0);
        assert_eq!(smeared_ls1b_up(0x8000000000000000), 0x8000000000000000);
        assert_eq!(smeared_ls1b_down(0b1010000), 0b1011111);
        assert_eq!(pop_count(0), 0);
        assert_eq!(pop_count(0b1001101), 4);
        assert_eq!(pop_count(0xffffffffffffffff), 64);
    }

    #[test]
    fn test_bitscan() {
        assert_eq!(bitscan_forward(0b1001101), 0);
        assert_eq!(bitscan_forward(0b1001000), 3);
        assert_eq!(bitscan_forward(0xf000000000000000), 60);
        let mut b = 0b1100100;
        assert_eq!(bitscan_forward_and_reset(&mut b), 2);
        assert_eq!(b, 0b1100000);
        assert_eq!(bitscan_1bit(0b1000), bitscan_forward(0b1000));
    }

}
