use sha2::{Digest, Sha256};

const SALT: &str = "app-launcher-v1-2026";

// A key is valid if SHA256(normalized_key + SALT) starts with "00".
// ~1/256 random keys pass by chance. Generate valid keys with generate_key().
pub fn validate_key(key: &str) -> bool {
    let normalized = key.replace('-', "").to_uppercase();
    if normalized.len() != 16 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let input = format!("{}{}", normalized, SALT);
    let hash = Sha256::digest(input.as_bytes());
    hash[0] == 0
}

pub fn is_licensed(license_key: &Option<String>) -> bool {
    match license_key {
        Some(key) => validate_key(key),
        None => false,
    }
}

pub fn group_limit(license_key: &Option<String>) -> usize {
    if is_licensed(license_key) { usize::MAX } else { 2 }
}

#[allow(dead_code)]
pub fn generate_key() -> String {
    use uuid::Uuid;
    loop {
        let raw = Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase();
        let formatted = format!(
            "{}-{}-{}-{}",
            &raw[0..4], &raw[4..8], &raw[8..12], &raw[12..16]
        );
        if validate_key(&formatted) {
            return formatted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_key_wrong_length() {
        assert!(!validate_key("TOO-SHORT"));
    }

    #[test]
    fn test_invalid_key_non_hex() {
        assert!(!validate_key("ZZZZ-ZZZZ-ZZZZ-ZZZZ"));
    }

    #[test]
    fn test_generated_key_is_valid() {
        let key = generate_key();
        assert!(validate_key(&key), "Generated key should be valid: {}", key);
    }

    #[test]
    fn test_group_limit_unlicensed() {
        assert_eq!(group_limit(&None), 2);
    }

    #[test]
    fn test_group_limit_licensed() {
        let key = generate_key();
        assert_eq!(group_limit(&Some(key)), usize::MAX);
    }

    #[test]
    fn test_is_licensed_none() {
        assert!(!is_licensed(&None));
    }
}
