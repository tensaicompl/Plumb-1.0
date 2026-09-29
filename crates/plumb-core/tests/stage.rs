use plumb_core::{StageId, UnknownStageId};
use serde_json::json;

#[test]
fn all_thirteen_stages_round_trip_exactly() {
    let expected = [
        "S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12",
    ];
    assert_eq!(StageId::ALL.len(), 13);
    for (stage, text) in StageId::ALL.into_iter().zip(expected) {
        assert_eq!(stage.as_str(), text);
        assert_eq!(stage.to_string(), text);
        assert_eq!(text.parse::<StageId>().unwrap(), stage);
        assert_eq!(serde_json::to_value(stage).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<StageId>(json!(text)).unwrap(),
            stage
        );
    }
}

#[test]
fn unknown_and_lowercase_stages_are_rejected() {
    for bad in [
        "s0", "s12", "S13", "S01", "S", "", " S1", "S1 ", "stage-1", "S-1",
    ] {
        assert_eq!(bad.parse::<StageId>(), Err(UnknownStageId(bad.to_owned())));
        assert!(serde_json::from_value::<StageId>(json!(bad)).is_err());
    }
    assert!(serde_json::from_value::<StageId>(json!(1)).is_err());
}
