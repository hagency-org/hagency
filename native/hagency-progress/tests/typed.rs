use hagency_progress::*;
use serde_json::json;
#[test]
fn native_progress_typed_tools_receipts_and_terminal_evidence() {
    let run = RunId::new("typed-host-run".into()).unwrap();
    let mut state = Accumulator::new(
        run.clone(),
        Filter::parse(&json!({"events":["step","done"]}), None).unwrap(),
    );
    state.start(&run, 1, 10).unwrap();
    assert_eq!(state.start(&run, 1, 0).unwrap(), Observation::Duplicate);
    assert_eq!(
        state.observe_tool(&run, 2, 10, ToolEvent::updated("a", ToolState::Completed)),
        Err(Error::Order)
    );
    state
        .observe_tool(
            &run,
            2,
            10,
            ToolEvent::started("a", "Bash", ToolState::Pending),
        )
        .unwrap();
    assert_eq!(
        state
            .observe_tool(
                &run,
                2,
                0,
                ToolEvent::started("a", "Bash", ToolState::Pending)
            )
            .unwrap(),
        Observation::Duplicate
    );
    assert_eq!(
        state.observe_tool(
            &run,
            2,
            10,
            ToolEvent::started("a", "Edit", ToolState::Pending)
        ),
        Err(Error::Conflict)
    );
    let attempt = state.claim(&run, 10).unwrap().unwrap();
    assert!(attempt.text().contains("1 attempt pending"));
    state
        .settle(&run, attempt.id(), 11, AttemptOutcome::ObservedAccepted)
        .unwrap();
    state
        .observe_tool(&run, 3, 12, ToolEvent::updated("a", ToolState::Completed))
        .unwrap();
    state
        .observe_tool(&run, 4, 13, ToolEvent::updated("a", ToolState::Completed))
        .unwrap();
    assert_eq!(
        state.observe_tool(&run, 5, 13, ToolEvent::updated("a", ToolState::Failed)),
        Err(Error::Conflict)
    );
    state
        .observe_tool(
            &run,
            5,
            13,
            ToolEvent::started("b", "Edit", ToolState::Failed),
        )
        .unwrap();
    state
        .observe_tool(
            &run,
            6,
            14,
            ToolEvent::started("c", "PrivateToolName", ToolState::Unknown),
        )
        .unwrap();
    state.finish(&run, 7, 15, None).unwrap();
    assert_eq!(
        state.summary().unwrap().as_deref(),
        Some("finished — ran commands, 1 failed, 1 attempt unresolved")
    );
    let last = state.claim(&run, 60_011).unwrap().unwrap();
    assert_eq!(
        last.text(),
        "⏳ finished — ran commands, 1 failed, 1 attempt unresolved"
    );
}
#[test]
fn native_progress_typed_tools_filter_shape_and_domains() {
    let run = RunId::new("typed-filter".into()).unwrap();
    let filter = Filter::parse(&json!({"tools":{"exclude":["Bash"]}}), None).unwrap();
    let mut state = Accumulator::new(run.clone(), filter);
    assert_eq!(
        state.observe_tool(
            &run,
            1,
            0,
            ToolEvent::started("bad\n", "Edit", ToolState::Pending)
        ),
        Err(Error::Shape)
    );
    state
        .observe_tool(
            &run,
            1,
            0,
            ToolEvent::started("a", "Bash", ToolState::Failed),
        )
        .unwrap();
    assert_eq!(state.observe_acp(&run,1,0,&json!({"sessionUpdate":"tool_call","toolCallId":"a","kind":"execute","status":"failed"})),Err(Error::Conflict));
    state
        .observe_tool(
            &run,
            2,
            1,
            ToolEvent::started("b", "Bash", ToolState::Unknown),
        )
        .unwrap();
    state
        .observe_tool(
            &run,
            3,
            2,
            ToolEvent::started("c", "Edit", ToolState::Completed),
        )
        .unwrap();
    state.finish(&run, 4, 3, None).unwrap();
    assert_eq!(
        state.summary().unwrap().as_deref(),
        Some("finished — edited")
    );
}
