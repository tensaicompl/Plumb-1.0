use std::collections::{BTreeMap, HashMap};

use plumb_core::{canonical_hash, to_canonical_json, CoreError, HashKind};
use proptest::prelude::*;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::{json, Value};

/// A JSON object that serializes its entries in exactly the given insertion order,
/// so canonicalization (not the container) is what must establish key order.
#[derive(Debug, Clone)]
struct InsertionOrdered(Vec<(String, Node)>);

#[derive(Debug, Clone)]
enum Node {
    Leaf(Value),
    Object(InsertionOrdered),
}

impl Serialize for InsertionOrdered {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Leaf(value) => value.serialize(serializer),
            Node::Object(object) => object.serialize(serializer),
        }
    }
}

fn canonical_string<T: Serialize>(value: &T) -> String {
    String::from_utf8(to_canonical_json(value).unwrap()).unwrap()
}

#[test]
fn keys_are_sorted_and_output_is_compact() {
    let value = InsertionOrdered(vec![
        ("zeta".into(), Node::Leaf(json!(1))),
        ("alpha".into(), Node::Leaf(json!([3, 2, 1]))),
        ("mid".into(), Node::Leaf(json!({"y": true, "x": null}))),
    ]);
    assert_eq!(
        canonical_string(&value),
        r#"{"alpha":[3,2,1],"mid":{"x":null,"y":true},"zeta":1}"#
    );
}

#[test]
fn arrays_keep_their_supplied_order() {
    // F0.1 does not reorder semantically unordered arrays (plan §6.3).
    assert_eq!(canonical_string(&json!(["b", "a"])), r#"["b","a"]"#);
}

#[test]
fn numbers_use_rfc8785_serialization() {
    let value: Value = serde_json::from_str(r#"[1.0, 2.50, 1e21, -0.0, 1e-7, 100]"#).unwrap();
    assert_eq!(canonical_string(&value), "[1,2.5,1e+21,0,1e-7,100]");
}

/// Plan §6.3 RFC 8785 conformance fixtures: `(name, input JSON text, exact expected bytes)`.
/// Expected bytes are fixed literals derived from RFC 8785 §3.2 (UTF-16 code-unit key order,
/// minimal escaping, ECMAScript numbers); non-ASCII output is spelled out as UTF-8 bytes.
const RFC8785_FIXTURES: &[(&str, &str, &[u8])] = &[
    (
        "ASCII keys",
        r#"{"b":1,"a":2,"aa":3,"B":4}"#,
        br#"{"B":4,"a":2,"aa":3,"b":1}"#,
    ),
    (
        "quote/backslash keys",
        r#"{"b\\":1,"b\"":2,"b":3}"#,
        br#"{"b":3,"b\"":2,"b\\":1}"#,
    ),
    (
        "control-char keys",
        r#"{"A":1,"\n":2,"\u001f":3}"#,
        br#"{"\n":2,"\u001f":3,"A":1}"#,
    ),
    (
        "BMP keys",
        r#"{"z":1,"\u00e9":2,"\u20ac":3}"#,
        b"{\"z\":1,\"\xC3\xA9\":2,\"\xE2\x82\xAC\":3}",
    ),
    (
        "non-BMP keys (UTF-16 order: U+1F600 before U+FB00)",
        r#"{"\ufb00":1,"\ud83d\ude00":2}"#,
        b"{\"\xF0\x9F\x98\x80\":2,\"\xEF\xAC\x80\":1}",
    ),
    (
        "string escaping",
        r#"{"s":"\u001f\u007f/\u00e9\u2028"}"#,
        b"{\"s\":\"\\u001f\x7F/\xC3\xA9\xE2\x80\xA8\"}",
    ),
    (
        "numbers",
        r#"[1.0,2.50,1e21,-0.0,1e-7,100,0.000001]"#,
        br#"[1,2.5,1e+21,0,1e-7,100,0.000001]"#,
    ),
];

#[test]
fn rfc8785_conformance_fixtures_produce_fixed_expected_bytes() {
    for (name, input, expected) in RFC8785_FIXTURES {
        let value: Value = serde_json::from_str(input).unwrap();
        assert_eq!(
            to_canonical_json(&value).unwrap(),
            *expected,
            "fixture {name:?}: output differs from the fixed RFC 8785 bytes"
        );
    }
}

#[test]
fn rfc8785_expected_outputs_are_fixed_points() {
    for (name, _, expected) in RFC8785_FIXTURES {
        let value: Value = serde_json::from_slice(expected).unwrap();
        assert_eq!(
            to_canonical_json(&value).unwrap(),
            *expected,
            "fixture {name:?}: canonical output is not a fixed point"
        );
    }
}

#[test]
fn opposite_insertion_orders_give_identical_bytes_and_hashes() {
    let forward = InsertionOrdered(vec![
        ("a".into(), Node::Leaf(json!(1))),
        ("b".into(), Node::Leaf(json!("two"))),
        ("c".into(), Node::Leaf(json!([true, null]))),
    ]);
    let mut reversed = forward.clone();
    reversed.0.reverse();
    assert_eq!(
        to_canonical_json(&forward).unwrap(),
        to_canonical_json(&reversed).unwrap()
    );
    for kind in [HashKind::Generic, HashKind::Semantic, HashKind::Evidence] {
        assert_eq!(
            canonical_hash(kind, &forward).unwrap(),
            canonical_hash(kind, &reversed).unwrap()
        );
    }
}

#[test]
fn hash_map_and_btree_map_canonicalize_identically() {
    let mut btree = BTreeMap::new();
    let mut hashed = HashMap::new();
    for i in 0..64 {
        btree.insert(format!("key{i}"), i);
        hashed.insert(format!("key{i}"), i);
    }
    assert_eq!(
        to_canonical_json(&btree).unwrap(),
        to_canonical_json(&hashed).unwrap()
    );
}

#[test]
fn repeated_canonicalization_is_byte_identical() {
    let value = json!({"b": {"d": [1, 2], "c": "x"}, "a": 0.5});
    let first = to_canonical_json(&value).unwrap();
    for _ in 0..10 {
        assert_eq!(to_canonical_json(&value).unwrap(), first);
    }
    let reparsed: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(to_canonical_json(&reparsed).unwrap(), first);
}

/// 2^53 - 1, the largest integer every I-JSON consumer represents exactly.
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

fn leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        // Only the I-JSON safe range canonicalizes (Fable #28/#29).
        (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).prop_map(Value::from),
        "[ -~]{0,12}".prop_map(Value::from),
    ]
}

fn key() -> impl Strategy<Value = String> {
    // Arbitrary Unicode keys, including escapes and non-BMP characters.
    any::<String>().prop_map(|s| s.chars().take(8).collect())
}

/// Entries of a JSON object in some insertion order.
type Entries<V> = Vec<(String, V)>;

/// Unique-keyed entries plus a shuffled permutation of the same entries.
fn permuted_entries<V: Clone + std::fmt::Debug + 'static>(
    value: impl Strategy<Value = V> + 'static,
) -> impl Strategy<Value = (Entries<V>, Entries<V>)> {
    prop::collection::btree_map(key(), value, 0..16).prop_flat_map(|map| {
        let entries: Entries<V> = map.into_iter().collect();
        (Just(entries.clone()), Just(entries).prop_shuffle())
    })
}

fn flat(entries: Entries<Value>) -> InsertionOrdered {
    InsertionOrdered(
        entries
            .into_iter()
            .map(|(k, v)| (k, Node::Leaf(v)))
            .collect(),
    )
}

proptest! {
    #[test]
    fn insertion_order_never_changes_canonical_bytes_or_hash((original, shuffled) in permuted_entries(leaf())) {
        let a = flat(original);
        let b = flat(shuffled);
        let bytes_a = to_canonical_json(&a).unwrap();
        prop_assert_eq!(&bytes_a, &to_canonical_json(&b).unwrap());
        // Canonical bytes parse back to exactly the supplied value (no precision loss).
        let reparsed: Value = serde_json::from_slice(&bytes_a).unwrap();
        prop_assert_eq!(reparsed, serde_json::to_value(&a).unwrap());
        prop_assert_eq!(&bytes_a, &to_canonical_json(&a).unwrap());
        for kind in [HashKind::Generic, HashKind::Semantic, HashKind::Evidence] {
            prop_assert_eq!(canonical_hash(kind, &a).unwrap(), canonical_hash(kind, &b).unwrap());
        }
    }

    #[test]
    fn nested_insertion_order_never_changes_canonical_bytes_or_hash(
        (outer, outer_shuffled) in permuted_entries(permuted_entries(leaf())),
    ) {
        let original = InsertionOrdered(
            outer.into_iter().map(|(k, (inner, _))| (k, Node::Object(flat(inner)))).collect(),
        );
        let shuffled = InsertionOrdered(
            outer_shuffled.into_iter().map(|(k, (_, inner))| (k, Node::Object(flat(inner)))).collect(),
        );
        prop_assert_eq!(to_canonical_json(&original).unwrap(), to_canonical_json(&shuffled).unwrap());
        prop_assert_eq!(
            canonical_hash(HashKind::Semantic, &original).unwrap(),
            canonical_hash(HashKind::Semantic, &shuffled).unwrap()
        );
    }
}

/// A map that emits exactly the given (key, value) entries, duplicates included.
struct RawEntries<K, V>(Vec<(K, V)>);

impl<K: Serialize, V: Serialize> Serialize for RawEntries<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Emits the integer key `1` and the string key `"1"`: both are the member name "1".
struct StringifiedKeyCollision;

impl Serialize for StringifiedKeyCollision {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(&1_u32, "integer key")?;
        map.serialize_entry("1", "string key")?;
        map.end()
    }
}

fn assert_canonicalization_error<T: Serialize + ?Sized>(value: &T, what: &str) {
    match to_canonical_json(value) {
        Err(CoreError::Canonicalization(_)) => {}
        other => panic!("{what}: expected CoreError::Canonicalization, got {other:?}"),
    }
    for kind in [HashKind::Generic, HashKind::Semantic, HashKind::Evidence] {
        assert!(
            matches!(canonical_hash(kind, value), Err(CoreError::Canonicalization(_))),
            "{what}: canonical_hash({kind:?}) must fail too"
        );
    }
}

#[test]
fn duplicate_member_names_are_rejected() {
    // Fable #27: keeping the first of two members would hash distinct inputs identically.
    let duplicate = InsertionOrdered(vec![
        ("a".into(), Node::Leaf(json!(1))),
        ("a".into(), Node::Leaf(json!(2))),
    ]);
    assert_canonicalization_error(&duplicate, "duplicate top-level member");
    let nested = json!({"outer": [1, 2]});
    let nested = InsertionOrdered(vec![
        ("x".into(), Node::Leaf(nested)),
        (
            "y".into(),
            Node::Object(InsertionOrdered(vec![
                ("k".into(), Node::Leaf(json!(null))),
                ("k".into(), Node::Leaf(json!(null))),
            ])),
        ),
    ]);
    assert_canonicalization_error(&nested, "duplicate nested member");
    assert_canonicalization_error(&StringifiedKeyCollision, "stringified key collision");
}

#[test]
fn integers_outside_the_safe_range_are_rejected() {
    // Fable #28: integers beyond 2^53 - 1 would be widened to f64 and collide.
    let max = MAX_SAFE_INTEGER;
    assert_eq!(canonical_string(&json!([max, -max])), format!("[{max},-{max}]"));
    assert_canonicalization_error(&(max + 1), "2^53");
    assert_canonicalization_error(&(-max - 1), "-2^53");
    assert_canonicalization_error(&u64::MAX, "u64::MAX");
    assert_canonicalization_error(&i64::MIN, "i64::MIN");
    assert_canonicalization_error(&(u128::from(u64::MAX) + 1), "u128");
    assert_canonicalization_error(&i128::MIN, "i128::MIN");
    assert_canonicalization_error(&json!({"n": [1, u64::MAX]}), "nested u64::MAX");
    // Distinct out-of-range values must never canonicalize at all, let alone identically.
    assert_canonicalization_error(&9_007_199_254_740_993_u64, "2^53 + 1");
}

#[test]
fn integer_map_keys_outside_the_safe_range_are_rejected() {
    let max = MAX_SAFE_INTEGER;
    let ok: BTreeMap<i64, bool> = [(max, true), (-max, false)].into();
    assert_eq!(
        canonical_string(&ok),
        format!(r#"{{"-{max}":false,"{max}":true}}"#)
    );
    let over: BTreeMap<u64, bool> = [(1 << 53, true)].into();
    assert_canonicalization_error(&over, "integer key 2^53");
    let under: BTreeMap<i64, bool> = [(-max - 1, true)].into();
    assert_canonicalization_error(&under, "integer key -2^53");
}

#[test]
fn non_finite_numbers_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_canonicalization_error(&value, "non-finite f64");
        assert_canonicalization_error(&vec![1.0, value], "non-finite f64 in an array");
    }
    assert_canonicalization_error(&f32::NAN, "f32 NaN");
}

#[test]
fn raw_values_are_rejected() {
    let raw = serde_json::value::RawValue::from_string(r#"{"b":1,"a":2}"#.to_owned()).unwrap();
    assert_canonicalization_error(&raw, "RawValue");
    assert_canonicalization_error(&RawEntries(vec![("raw", &*raw)]), "nested RawValue");
}

#[test]
fn non_string_keys_are_rejected() {
    assert_canonicalization_error(&RawEntries(vec![((1, 2), 0)]), "tuple key");
    assert_canonicalization_error(&RawEntries(vec![(true, 0)]), "bool key");
    assert_canonicalization_error(&RawEntries(vec![(1.5_f64, 0)]), "float key");
    assert_canonicalization_error(&RawEntries(vec![(vec!["a"], 0)]), "sequence key");
    assert_canonicalization_error(&RawEntries(vec![((), 0)]), "unit key");
}
