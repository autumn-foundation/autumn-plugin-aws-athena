use super::QueryState;

#[test]
fn parses_each_athena_state() {
    assert_eq!(QueryState::parse("QUEUED"), QueryState::Queued);
    assert_eq!(QueryState::parse("RUNNING"), QueryState::Running);
    assert_eq!(QueryState::parse("SUCCEEDED"), QueryState::Succeeded);
    assert_eq!(QueryState::parse("FAILED"), QueryState::Failed);
    assert_eq!(QueryState::parse("CANCELLED"), QueryState::Cancelled);
}

#[test]
fn keeps_an_unknown_state_name() {
    assert_eq!(
        QueryState::parse("PAUSED"),
        QueryState::Unknown("PAUSED".to_owned())
    );
}

#[test]
fn only_final_states_are_terminal() {
    assert!(!QueryState::Queued.is_terminal());
    assert!(!QueryState::Running.is_terminal());
    assert!(QueryState::Succeeded.is_terminal());
    assert!(QueryState::Failed.is_terminal());
    assert!(QueryState::Cancelled.is_terminal());
    // Fail safe: keep polling. The deadline stops the loop.
    assert!(!QueryState::Unknown("PAUSED".to_owned()).is_terminal());
}
