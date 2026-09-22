use super::*;

/// Exact pool status/action decision table over independent full-width
/// u32 status and action ids, with no assumptions: invalid status and
/// action values are genuine inputs and keep the predicate's actual
/// behavior (they pass this gate and remain subject to RequestType
/// parsing elsewhere).
#[kani::proof]
fn prove_pool_action_status_table() {
    let status: u32 = kani::any();
    let action: u32 = kani::any();
    let disallowed = is_action_disallowed(status, action);

    if status <= 1 {
        assert!(!disallowed);
    } else if status <= 3 {
        assert_eq!(disallowed, action == 4 || action == 9);
    } else {
        assert_eq!(
            disallowed,
            action == 4 || action == 9 || action == 2 || action == 0
        );
    }

    // non-vacuity witnesses on the documented gate boundaries
    assert!(is_action_disallowed(2, 4));
    assert!(is_action_disallowed(4, 0));
    assert!(!is_action_disallowed(1, 4));
    assert!(!is_action_disallowed(3, 0));
}
