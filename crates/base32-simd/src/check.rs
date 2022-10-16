use crate::decode::{decode_bits, decode_extra};
use crate::decode::{BASE32HEX_TABLE, BASE32_TABLE};
use crate::Error;

use vsimd::base32::{Kind, BASE32HEX_ALSW_CHECK_X2, BASE32_ALSW_CHECK_X2};
use vsimd::tools::{slice, slice_parts};
use vsimd::SIMD256;

use core::ptr::null_mut;

#[inline(always)]
pub fn check_fallback(src: &[u8], kind: Kind) -> Result<(), Error> {
    let table = match kind {
        Kind::Base32 => BASE32_TABLE.as_ptr(),
        Kind::Base32Hex => BASE32HEX_TABLE.as_ptr(),
    };

    unsafe {
        let (mut src, mut len) = slice_parts(src);

        let end = src.add(len / 8 * 8);
        while src < end {
            let (_, flag) = decode_bits::<8>(src, table);
            ensure!(flag != 0xff);
            src = src.add(8);
        }
        len %= 8;

        decode_extra::<false>(src, len, null_mut(), table)
    }
}

#[inline(always)]
pub fn check_simd<S: SIMD256>(s: S, src: &[u8], kind: Kind) -> Result<(), Error> {
    let check_lut = match kind {
        Kind::Base32 => BASE32_ALSW_CHECK_X2,
        Kind::Base32Hex => BASE32HEX_ALSW_CHECK_X2,
    };

    unsafe {
        let (mut src, mut len) = slice_parts(src);

        let end = src.add(len / 32 * 32);
        while src < end {
            let x = s.v256_load_unaligned(src);

            let is_valid = vsimd::base32::check_ascii32(s, x, check_lut);
            ensure!(is_valid);

            src = src.add(32);
        }
        len %= 32;

        check_fallback(slice(src, len), kind)
    }
}
