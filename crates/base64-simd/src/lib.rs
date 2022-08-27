//! SIMD-accelerated base64 encoding and decoding.
//!
//! # Examples
//!
//! ```
//! use base64_simd::Base64;
//!
//! let bytes = b"hello world";
//! let base64 = Base64::STANDARD;
//!
//! let encoded = base64.encode_to_boxed_str(bytes);
//! assert_eq!(&*encoded, "aGVsbG8gd29ybGQ=");
//!
//! let decoded = base64.decode_to_boxed_bytes(encoded.as_bytes()).unwrap();
//! assert_eq!(&*decoded, bytes);
//! ```
//!

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![cfg_attr(feature = "unstable", feature(arm_target_feature))]
#![cfg_attr(docsrs, feature(doc_cfg))]
//
#![deny(
    missing_debug_implementations,
    missing_docs,
    clippy::all,
    clippy::cargo,
    clippy::missing_inline_in_public_items,
    clippy::must_use_candidate
)]
#![warn(clippy::todo)]

#[cfg(feature = "alloc")]
extern crate alloc;

mod error;
pub use self::error::Error;

mod spec;

mod decode;
mod encode;
mod forgiving;

mod multiversion;

#[cfg(test)]
mod tests;

pub(crate) use simd_abstraction::common::ascii as sa_ascii;

pub use outref::OutRef;

// -------------------------------------------------------------------------------------------------

use simd_abstraction::item_group;
use simd_abstraction::tools::slice_mut;

#[cfg(feature = "alloc")]
item_group!(
    use alloc::boxed::Box;
    use simd_abstraction::tools::{alloc_uninit_bytes, assume_init};
);

#[derive(Debug)]
enum Base64Kind {
    Standard,
    UrlSafe,
}

const STANDARD_CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const URL_SAFE_CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64 variants
///
/// + [`Base64::STANDARD`](crate::Base64::STANDARD)
/// + [`Base64::STANDARD_NO_PAD`](crate::Base64::STANDARD_NO_PAD)
/// + [`Base64::URL_SAFE`](crate::Base64::URL_SAFE)
/// + [`Base64::URL_SAFE_NO_PAD`](crate::Base64::URL_SAFE_NO_PAD)
///
#[derive(Debug)]
pub struct Base64 {
    kind: Base64Kind,
    padding: bool,
}

impl Base64 {
    /// Standard charset with padding.
    pub const STANDARD: Self = Self {
        kind: Base64Kind::Standard,
        padding: true,
    };

    /// Standard charset without padding.
    pub const STANDARD_NO_PAD: Self = Self {
        kind: Base64Kind::Standard,
        padding: false,
    };

    /// URL-safe charset with padding.
    pub const URL_SAFE: Self = Self {
        kind: Base64Kind::UrlSafe,
        padding: true,
    };

    /// URL-safe charset without padding.
    pub const URL_SAFE_NO_PAD: Self = Self {
        kind: Base64Kind::UrlSafe,
        padding: false,
    };

    /// Returns the character set used for encoding.
    #[inline]
    #[must_use]
    pub const fn charset(&self) -> &[u8; 64] {
        match self.kind {
            Base64Kind::Standard => STANDARD_CHARSET,
            Base64Kind::UrlSafe => URL_SAFE_CHARSET,
        }
    }

    /// Calculates the encoded length.
    ///
    /// # Panics
    /// This function will panics if `n > isize::MAX`.
    #[inline]
    #[must_use]
    pub const fn encoded_length(&self, n: usize) -> usize {
        assert!(n <= usize::MAX / 2);
        unsafe { crate::encode::encoded_length_unchecked(n, self.padding) }
    }

    /// Estimates the decoded length.
    ///
    /// The result is an upper bound which can be used for allocation.
    #[inline]
    #[must_use]
    pub const fn estimated_decoded_length(&self, n: usize) -> usize {
        if n % 4 == 0 {
            n / 4 * 3
        } else {
            (n / 4 + 1) * 3
        }
    }

    /// Calculates the decoded length.
    ///
    /// The result is a precise value which can be used for allocation.
    #[inline]
    pub fn decoded_length(&self, data: &[u8]) -> Result<usize, Error> {
        let (_, m) = crate::decode::decoded_length(data, self.padding)?;
        Ok(m)
    }

    /// Encodes `src` and writes to `dst`.
    ///
    /// # Panics
    /// This function will panic if the length of `dst` is not enough.
    #[inline]
    #[must_use]
    pub fn encode<'s, 'd>(&'_ self, src: &'s [u8], mut dst: OutRef<'d, [u8]>) -> &'d mut [u8] {
        unsafe {
            let m = crate::encode::encoded_length_unchecked(src.len(), self.padding);
            assert!(dst.len() >= m);

            let dst = dst.as_mut_ptr();
            crate::multiversion::encode::auto_indirect(self, src, dst);

            slice_mut(dst, m)
        }
    }

    /// Encodes `src` to `dst` and returns [`&mut str`](str).
    ///
    /// # Panics
    /// This function will panic if the length of `dst` is not enough.
    #[inline]
    #[must_use]
    pub fn encode_as_str<'s, 'd>(&'_ self, src: &'s [u8], dst: OutRef<'d, [u8]>) -> &'d mut str {
        let ans = self.encode(src, dst);
        unsafe { core::str::from_utf8_unchecked_mut(ans) }
    }

    /// Decodes `src` and writes to `dst`.
    ///
    /// # Errors
    /// This function returns `Err` if the content of `src` is invalid.
    ///
    /// # Panics
    /// This function will panic if the length of `dst` is not enough.
    #[inline]
    pub fn decode<'s, 'd>(&'_ self, src: &'s [u8], mut dst: OutRef<'d, [u8]>) -> Result<&'d mut [u8], Error> {
        unsafe {
            let (n, m) = crate::decode::decoded_length(src, self.padding)?;

            assert!(dst.len() >= m);

            let src = src.as_ptr();
            let dst = dst.as_mut_ptr();
            crate::multiversion::decode::auto_indirect(self, n, m, src, dst)?;

            Ok(slice_mut(dst, m))
        }
    }

    /// Decodes `data` and writes inplace.
    ///
    /// # Errors
    /// This function returns `Err` if the content of `data` is invalid.
    #[inline]
    pub fn decode_inplace<'d>(&'_ self, data: &'d mut [u8]) -> Result<&'d mut [u8], Error> {
        unsafe {
            let (n, m) = crate::decode::decoded_length(data, self.padding)?;

            let dst: *mut u8 = data.as_mut_ptr();
            let src: *const u8 = dst;
            crate::multiversion::decode::auto_indirect(self, n, m, src, dst)?;

            Ok(slice_mut(dst, m))
        }
    }

    /// Encodes `data` and returns [`Box<str>`]
    ///
    /// # Panics
    /// This function will panics if the encoded length of `data` is greater than `isize::MAX`.
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    #[cfg(feature = "alloc")]
    #[inline]
    #[must_use]
    pub fn encode_to_boxed_str(&self, data: &[u8]) -> Box<str> {
        if data.is_empty() {
            return Box::from("");
        }

        unsafe {
            let m = crate::encode::encoded_length_unchecked(data.len(), self.padding);
            assert!(m <= usize::MAX / 2);

            let mut uninit_buf = alloc_uninit_bytes(m);

            let dst: *mut u8 = uninit_buf.as_mut_ptr().cast();
            crate::multiversion::encode::auto_indirect(self, data, dst);

            let len = uninit_buf.len();
            let ptr = Box::into_raw(uninit_buf).cast::<u8>();
            Box::from_raw(core::str::from_utf8_unchecked_mut(slice_mut(ptr, len)))
        }
    }

    /// Decodes `data` and returns [`Box<[u8]>`](Box)
    ///
    /// # Errors
    /// This function returns `Err` if the content of `data` is invalid.
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    #[cfg(feature = "alloc")]
    #[inline]
    pub fn decode_to_boxed_bytes(&self, data: &[u8]) -> Result<Box<[u8]>, Error> {
        if data.is_empty() {
            return Ok(Box::from([]));
        }

        unsafe {
            let (n, m) = crate::decode::decoded_length(data, self.padding)?;

            // safety: 0 < m < isize::MAX
            let mut uninit_buf = alloc_uninit_bytes(m);

            let dst: *mut u8 = uninit_buf.as_mut_ptr().cast();
            let src: *const u8 = data.as_ptr();
            crate::multiversion::decode::auto_indirect(self, n, m, src, dst)?;

            Ok(assume_init(uninit_buf))
        }
    }

    /// Forgiving decodes `data` and writes inplace.
    ///
    /// See <https://infra.spec.whatwg.org/#forgiving-base64>
    ///
    /// # Errors
    /// This function returns `Err` if the content of `data` is invalid.
    #[inline]
    pub fn forgiving_decode_inplace(data: &mut [u8]) -> Result<&mut [u8], Error> {
        let data = crate::forgiving::normalize(data);
        Self::STANDARD_NO_PAD.decode_inplace(data)
    }
}
