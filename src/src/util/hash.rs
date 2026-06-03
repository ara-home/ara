/// Compute a SHA-256 hash of the given bytes.
pub fn compute(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}

/// Encode a 32-byte hash as a lowercase hex string.
pub fn hex_encode(hash: &[u8; 32]) -> String {
    hex::encode(hash)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_compute_and_hex_encode() {
        let h = compute(b"hello");
        let encoded = hex_encode(&h);
        assert_eq!(
            encoded,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
