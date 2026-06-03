#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("invalid hash length: expected {expected}, got {actual}")]
    InvalidLength { expected: usize, actual: usize },
    #[error("invalid hex character")]
    InvalidHex,
}

const HASH_PREFIX: &str = "sha256:";

pub fn compute(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}

pub fn hex_encode(hash: &[u8; 32]) -> String {
    hex::encode(hash)
}

pub fn format(hash: &[u8; 32]) -> String {
    format!("{}{}", HASH_PREFIX, hex::encode(hash))
}

pub fn from_hex(hex: &str) -> Result<[u8; 32], HashError> {
    let expected = 64;
    if hex.len() != expected {
        return Err(HashError::InvalidLength {
            expected,
            actual: hex.len(),
        });
    }
    let mut result = [0u8; 32];
    hex::decode_to_slice(hex, &mut result).map_err(|_| HashError::InvalidHex)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
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
    fn test_format_with_prefix() {
        let h = compute(b"hello");
        let formatted = format(&h);
        assert!(formatted.starts_with("sha256:"));
        assert_eq!(formatted.len(), 64 + "sha256:".len());
    }

    #[test]
    fn test_from_hex_roundtrip() {
        let h = compute(b"hello");
        let encoded = hex_encode(&h);
        let decoded = from_hex(&encoded).unwrap();
        assert_eq!(h, decoded);
    }

    #[test]
    fn test_from_hex_invalid_length() {
        let result = from_hex("abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_hex_invalid_chars() {
        let result = from_hex("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
        assert!(result.is_err());
    }
}
