//! Shared authorization policy checks for API request handling.

/// Errors produced by UI run-ownership checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRunOwnerPolicyError {
    MissingUserIdentity,
    OwnerMismatch,
}

/// Enforce UI run ownership for user-session requests.
///
/// Returns `Ok(())` when the request should be allowed.
///
/// # Errors
///
/// Returns `UiRunOwnerPolicyError::MissingUserIdentity` when a UI JWT request
/// does not carry a resolved user ID, and `UiRunOwnerPolicyError::OwnerMismatch`
/// when the run owner differs from the requesting user.
pub fn enforce_ui_run_owner(
    is_dev_mode: bool,
    is_platform_admin: bool,
    is_ui_jwt: bool,
    requester_user_id: Option<&str>,
    run_owner_user_id: Option<&str>,
) -> Result<(), UiRunOwnerPolicyError> {
    if is_dev_mode || is_platform_admin || !is_ui_jwt {
        return Ok(());
    }

    let requester_user_id = requester_user_id.ok_or(UiRunOwnerPolicyError::MissingUserIdentity)?;
    if let Some(owner_user_id) = run_owner_user_id
        && owner_user_id != requester_user_id
    {
        return Err(UiRunOwnerPolicyError::OwnerMismatch);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_non_ui_jwt_paths() {
        assert_eq!(
            enforce_ui_run_owner(false, false, false, None, Some("owner-1")),
            Ok(())
        );
    }

    #[test]
    fn allows_owner_and_denies_non_owner() {
        assert_eq!(
            enforce_ui_run_owner(false, false, true, Some("owner-1"), Some("owner-1")),
            Ok(())
        );
        assert_eq!(
            enforce_ui_run_owner(false, false, true, Some("user-2"), Some("owner-1")),
            Err(UiRunOwnerPolicyError::OwnerMismatch)
        );
    }

    #[test]
    fn requires_identity_for_ui_jwt_requests() {
        assert_eq!(
            enforce_ui_run_owner(false, false, true, None, Some("owner-1")),
            Err(UiRunOwnerPolicyError::MissingUserIdentity)
        );
    }
}
