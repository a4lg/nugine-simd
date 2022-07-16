use simd_abstraction::ascii::AsciiCase;
use simd_abstraction::hex::{encode_u8x16, ENCODE_LOWER_LUT, ENCODE_UPPER_LUT};
use simd_abstraction::tools::{read, Bytes16, Load};
use simd_abstraction::traits::SIMD256;

const fn full_table(table: &[u8; 16]) -> [u16; 256] {
    let mut buf = [0; 256];
    let mut i = 0;
    while i < 256 {
        let hi = table[i >> 4];
        let lo = table[i & 0xf];
        buf[i] = u16::from_ne_bytes([hi, lo]);
        i += 1;
    }
    buf
}

const UPPER_TABLE: &[u8; 16] = b"0123456789ABCDEF";
const LOWER_TABLE: &[u8; 16] = b"0123456789abcdef";

const FULL_LOWER_TABLE: &[u16; 256] = &full_table(LOWER_TABLE);
const FULL_UPPER_TABLE: &[u16; 256] = &full_table(UPPER_TABLE);

#[inline(always)]
pub unsafe fn encode_raw_fallback(src: &[u8], mut dst: *mut u8, case: AsciiCase) {
    let (n, src) = (src.len(), src.as_ptr());
    let table = match case {
        AsciiCase::Lower => FULL_LOWER_TABLE.as_ptr(),
        AsciiCase::Upper => FULL_UPPER_TABLE.as_ptr(),
    };
    for i in 0..n {
        let x = read(src, i);
        let y = read(table, x as usize);
        dst.cast::<u16>().write_unaligned(y);
        dst = dst.add(2);
    }
}

#[inline(always)]
pub unsafe fn encode_raw_simd<S: SIMD256>(s: S, src: &[u8], dst: *mut u8, case: AsciiCase) {
    let simd_lut = match case {
        AsciiCase::Lower => ENCODE_LOWER_LUT,
        AsciiCase::Upper => ENCODE_UPPER_LUT,
    };
    let mut cur: *mut u8 = dst;
    let (prefix, chunks, suffix) = src.align_to::<Bytes16>();

    if !prefix.is_empty() {
        encode_raw_fallback(prefix, cur, case);
        cur = cur.add(prefix.len() * 2);
    }

    let lut = s.load(simd_lut);
    for chunk in chunks {
        let ans = encode_u8x16(s, s.load(chunk), lut);
        s.v256_store_unaligned(cur, ans);
        cur = cur.add(32);
    }

    if !suffix.is_empty() {
        encode_raw_fallback(suffix, cur, case);
    }
}