//! FNV-1a, vendored in ten lines from gqls `src/semantic/cache.rs`.
//!
//! Used in place of `DefaultHasher` because these hashes are an **on-disk
//! format**, not an in-memory one: a `norm_hash` is the cache key for an LLM
//! summary that cost real money, and `DefaultHasher`'s algorithm is explicitly
//! unspecified across Rust releases. A toolchain bump would silently orphan
//! every summary in the database.
//!
//! Not cryptographic. The only property required is that an unrelated edit is
//! overwhelmingly unlikely to land on the same value.

pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Folded in between concatenated fields so adjacent values cannot alias
/// (`"ab" + "c"` must not hash like `"a" + "bc"`). `0xff` is never a byte of
/// valid UTF-8, so it cannot occur inside a name or a literal.
pub(crate) const SEP: &[u8] = &[0xff];

#[cfg(test)]
mod tests {
    use super::*;

    /// Frozen: these values are an on-disk format. If this test fails, every
    /// stored summary key has moved and the change needs a schema bump, not a
    /// new expectation.
    #[test]
    fn the_hash_is_frozen() {
        assert_eq!(fnv1a(FNV_OFFSET, b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(FNV_OFFSET, b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(FNV_OFFSET, b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn a_separator_stops_adjacent_fields_aliasing() {
        let ab_c = fnv1a(fnv1a(FNV_OFFSET, b"ab"), SEP);
        let a_bc = fnv1a(fnv1a(FNV_OFFSET, b"a"), SEP);
        assert_ne!(fnv1a(ab_c, b"c"), fnv1a(a_bc, b"bc"));
    }
}
