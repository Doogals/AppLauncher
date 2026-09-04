pub fn is_licensed(license_key: &Option<String>, instance_id: &Option<String>) -> bool {
    license_key.is_some() && instance_id.is_some()
}

pub fn group_limit(license_key: &Option<String>, instance_id: &Option<String>) -> usize {
    if is_licensed(license_key, instance_id) { usize::MAX } else { 1 }
}

/// Whether groups may be flagged to launch when Windows starts TakeOff.
///
/// A named function rather than an inline `is_licensed` call so every paid
/// feature is greppable from one place as the list grows. It must be consulted
/// on the startup path itself, not only when drawing the toggle: `config.json`
/// is plain text in `%LOCALAPPDATA%`, so a UI-only gate stops nobody.
pub fn startup_groups_allowed(
    license_key: &Option<String>,
    instance_id: &Option<String>,
) -> bool {
    is_licensed(license_key, instance_id)
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

    #[test]
    fn test_startup_groups_require_a_full_license() {
        let key = Some("key".to_string());
        let inst = Some("inst".to_string());
        assert!(!startup_groups_allowed(&None, &None));
        // A key with no instance id means activation never completed — this is
        // the state a revoked license is left in by clear_license_local.
        assert!(!startup_groups_allowed(&key, &None));
        assert!(!startup_groups_allowed(&None, &inst));
        assert!(startup_groups_allowed(&key, &inst));
    }
}
