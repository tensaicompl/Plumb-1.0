use plumb_core::{GateId, UnknownGateId};
use serde_json::json;

#[test]
fn thirteen_gates_in_dependency_order() {
    let expected = [
        "I0", "F1", "F2", "F3", "F4", "Q1", "A1", "A2", "A3", "A4", "D1", "D2", "C1",
    ];
    assert_eq!(GateId::ALL.len(), 13);
    for (i, (gate, text)) in GateId::ALL.into_iter().zip(expected).enumerate() {
        assert_eq!(gate.as_str(), text);
        assert_eq!(gate.to_string(), text);
        assert_eq!(gate.ordinal(), i);
        assert_eq!(text.parse::<GateId>().unwrap(), gate);
        assert_eq!(serde_json::to_value(gate).unwrap(), json!(text));
        assert_eq!(serde_json::from_value::<GateId>(json!(text)).unwrap(), gate);
    }
    let mut sorted = GateId::ALL;
    sorted.sort();
    assert_eq!(sorted, GateId::ALL);
}

#[test]
fn unknown_and_lowercase_gates_are_rejected() {
    for bad in ["i0", "f1", "c1", "A0", "G1", "I1", "", " F1", "F1 ", "S0"] {
        assert_eq!(bad.parse::<GateId>(), Err(UnknownGateId(bad.to_owned())));
        assert!(serde_json::from_value::<GateId>(json!(bad)).is_err());
    }
}
