use sha2::Digest;

/// Compute a SHA-256 hash of the given bytes.
pub fn compute(bytes: &[u8]) -> [u8; 32] {
    sha2::Sha256::digest(bytes).into()
}

/// Compute a SHA-512 hash of the given bytes.
pub fn compute_sha512(bytes: &[u8]) -> [u8; 64] {
    sha2::Sha512::digest(bytes).into()
}

/// Encode a 32-byte hash as a lowercase hex string.
pub fn hex_encode(hash: &[u8; 32]) -> String {
    hex::encode(hash)
}

/// Encode a 64-byte hash as a lowercase hex string.
pub fn hex_encode_64(hash: &[u8; 64]) -> String {
    hex::encode(hash)
}

/// Verify content against an SRI-style integrity string (e.g. `sha256-<hex>` or `sha512-<base64>`).
/// Returns `true` if the content matches the integrity value.
pub fn verify_integrity(content: &[u8], integrity: &str) -> bool {
    let (algo, expected) = match integrity.split_once('-') {
        Some((a, h)) => (a, h),
        None => return false,
    };
    match algo {
        "sha256" => {
            if expected.len() == 64 {
                let actual = hex_encode(&compute(content));
                actual == expected
            } else {
                false
            }
        }
        "sha512" => {
            use base64::Engine;
            if let Ok(expected_bytes) = base64::engine::general_purpose::STANDARD.decode(expected) {
                let actual = compute_sha512(content);
                actual[..] == expected_bytes[..]
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Format a SHA-256 hash as a `sha256-<hex>` string.
pub fn format_sha256(content: &[u8]) -> String {
    format!("sha256-{}", hex_encode(&compute(content)))
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

    #[test]
    fn test_verify_integrity_sha256_hex() {
        let content = b"hello";
        let integrity = "sha256-2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify_integrity(content, integrity));
        assert!(!verify_integrity(b"tampered", integrity));
    }

    #[test]
    fn test_verify_integrity_sha512_sri() {
        let content = b"hello";
        // Known sha512- base64 SRI for "hello" (computed correctly)
        let actual_512 = compute_sha512(content);
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(actual_512);
        let sri = format!("sha512-{b64}");
        assert!(verify_integrity(content, &sri));
    }

    #[test]
    fn test_format_sha256() {
        let formatted = format_sha256(b"hello");
        assert_eq!(
            formatted,
            "sha256-2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_verify_integrity_invalid_algo() {
        assert!(!verify_integrity(b"hello", "md5-deadbeef"));
    }

    #[test]
    fn test_verify_integrity_missing_dash() {
        assert!(!verify_integrity(b"hello", "sha256deadbeef"));
    }
}
