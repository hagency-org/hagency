use hagency_core::tasks::TaskState;
use serde::Deserialize;

#[test]
fn native_task_state_machine_matches_javascript() {
    #[derive(Deserialize)]
    struct Case {
        from: TaskState,
        to: TaskState,
        allowed: bool,
    }
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../fixtures/task-transitions.json")).unwrap();
    assert_eq!(cases.len(), 25);
    for case in cases {
        assert_eq!(
            case.from.permits(case.to),
            case.allowed,
            "{:?} -> {:?}",
            case.from,
            case.to
        );
    }
}
