use plumb_core::{CanonicalJson, HashKind};
use serde_json::{json, Value};

#[test]
fn canonical_json_is_transparent_on_the_wire() {
    let value = json!({"b": [1, "é", true], "a": {"z": null, "y": 1.5}});
    let wrapped = CanonicalJson::new(value.clone());
    assert_eq!(serde_json::to_value(&wrapped).unwrap(), value);
    let parsed: CanonicalJson = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(parsed, wrapped);
    assert_eq!(parsed.as_value(), &value);
    assert_eq!(parsed.into_value(), value);
    // Not wrapped in an object.
    let empty: CanonicalJson = serde_json::from_str("{}").unwrap();
    assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");
    let scalar: CanonicalJson = serde_json::from_str("42").unwrap();
    assert_eq!(scalar.as_value(), &Value::from(42));
}

#[test]
fn canonical_bytes_and_content_hash_are_fixed() {
    let wrapped: CanonicalJson =
        serde_json::from_str(r#"{ "b" : [1, "é", true], "a": {"z": null, "y": 1.50} }"#).unwrap();
    assert_eq!(
        wrapped.canonical_bytes().unwrap(),
        "{\"a\":{\"y\":1.5,\"z\":null},\"b\":[1,\"é\",true]}".as_bytes()
    );
    let hash = wrapped.content_hash().unwrap();
    assert_eq!(hash.kind(), HashKind::Generic);
    assert_eq!(
        hash.as_str(),
        "sha256:1a9f880c49a5f8020ec4adf4ac15fe6fdc5d1c2742825ac6832c52b037883fa5"
    );
    assert_eq!(
        CanonicalJson::new(json!({}))
            .content_hash()
            .unwrap()
            .as_str(),
        "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
}
