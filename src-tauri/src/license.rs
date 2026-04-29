pub fn is_licensed(license_key: &Option<String>, instance_id: &Option<String>) -> bool {
    license_key.is_some() && instance_id.is_some()
}

pub fn group_limit(license_key: &Option<String>, instance_id: &Option<String>) -> usize {
    if is_licensed(license_key, instance_id) { usize::MAX } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_licensed_requires_both_key_and_instance() {
        assert!(!is_licensed(&None, &None));
        assert!(!is_licensed(&Some("key".to_string()), &None));
        assert!(!is_licensed(&None, &Some("inst".to_string())));
        assert!(is_licensed(&Some("key".to_string()), &Some("inst".to_string())));
    }

    #[test]
    fn test_group_limit_unlicensed() {
        assert_eq!(group_limit(&None, &None), 1);
    }

    #[test]
    fn test_group_limit_licensed() {
        assert_eq!(
            group_limit(&Some("key".to_string()), &Some("inst".to_string())),
            usize::MAX
        );
    }
}
