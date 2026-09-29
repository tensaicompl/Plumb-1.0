use plumb_core::{canonical_hash, CoreError, Hash, HashKind};

// SHA-256 test vectors (FIPS 180-2): "" and "abc".
const EMPTY_HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_HEX: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn accepts_all_three_prefixes_with_64_lowercase_hex() {
    for (text, kind) in [
        (format!("sha256:{ABC_HEX}"), HashKind::Generic),
        (format!("psg:sha256:{ABC_HEX}"), HashKind::Semantic),
        (format!("ev:sha256:{ABC_HEX}"), HashKind::Evidence),
    ] {
        let hash: Hash = text.parse().unwrap();
        assert_eq!(hash.as_str(), text);
        assert_eq!(hash.to_string(), text);
        assert_eq!(hash.as_ref(), text);
        assert_eq!(hash.kind(), kind);
        assert_eq!(Hash::try_from(text.clone()).unwrap(), hash);
    }
}

#[test]
fn rejects_uppercase_hex() {
    let upper = ABC_HEX.to_uppercase();
    for text in [
        format!("sha256:{upper}"),
        format!("psg:sha256:{upper}"),
        format!("sha256:A{}", &ABC_HEX[1..]),
    ] {
        assert_eq!(
            text.parse::<Hash>(),
            Err(CoreError::InvalidHash(text.clone()))
        );
    }
}

#[test]
fn rejects_wrong_digest_length() {
    for text in [
        format!("sha256:{}", &ABC_HEX[..63]),
        format!("sha256:{ABC_HEX}0"),
        "sha256:".to_owned(),
        format!("ev:sha256:{}", &ABC_HEX[..32]),
    ] {
        assert!(text.parse::<Hash>().is_err(), "{text:?}");
    }
}

#[test]
fn rejects_unknown_prefixes_whitespace_and_trailing_material() {
    for text in [
        format!("sha512:{ABC_HEX}"),
        format!("SHA256:{ABC_HEX}"),
        format!("psg:{ABC_HEX}"),
        format!("psg:ev:sha256:{ABC_HEX}"),
        format!("sha256-{ABC_HEX}"),
        ABC_HEX.to_owned(),
        format!(" sha256:{ABC_HEX}"),
        format!("sha256:{ABC_HEX} "),
        format!("sha256:{ABC_HEX}\n"),
        format!("sha256:{ABC_HEX}:extra"),
        format!("sha256:{}g", &ABC_HEX[..63]),
        String::new(),
    ] {
        assert!(text.parse::<Hash>().is_err(), "{text:?}");
    }
}

#[test]
fn constructors_hash_bytes_with_the_right_prefix() {
    assert_eq!(
        Hash::content_sha256(b"abc").as_str(),
        format!("sha256:{ABC_HEX}")
    );
    assert_eq!(
        Hash::semantic_sha256(b"abc").as_str(),
        format!("psg:sha256:{ABC_HEX}")
    );
    assert_eq!(
        Hash::evidence_sha256(b"abc").as_str(),
        format!("ev:sha256:{ABC_HEX}")
    );
    assert_eq!(
        Hash::content_sha256(b"").as_str(),
        format!("sha256:{EMPTY_HEX}")
    );
    assert_eq!(Hash::content_sha256(b"abc").kind(), HashKind::Generic);
    assert_eq!(Hash::semantic_sha256(b"abc").kind(), HashKind::Semantic);
    assert_eq!(Hash::evidence_sha256(b"abc").kind(), HashKind::Evidence);
}

#[test]
fn constructors_are_deterministic_and_produce_valid_hashes() {
    let data = b"plumb deterministic input";
    assert_eq!(Hash::content_sha256(data), Hash::content_sha256(data));
    assert_ne!(Hash::content_sha256(data), Hash::content_sha256(b"other"));
    for hash in [
        Hash::content_sha256(data),
        Hash::semantic_sha256(data),
        Hash::evidence_sha256(data),
    ] {
        assert_eq!(hash.as_str().parse::<Hash>().unwrap(), hash);
    }
}

#[test]
fn serde_round_trip_and_validating_deserialization() {
    let hash = Hash::semantic_sha256(b"abc");
    let json = serde_json::to_string(&hash).unwrap();
    assert_eq!(json, format!("\"psg:sha256:{ABC_HEX}\""));
    assert_eq!(serde_json::from_str::<Hash>(&json).unwrap(), hash);
    for bad in [
        format!("\"sha256:{}\"", ABC_HEX.to_uppercase()),
        "\"md5:abc\"".to_owned(),
        "7".to_owned(),
    ] {
        assert!(serde_json::from_str::<Hash>(&bad).is_err(), "{bad}");
    }
}

#[test]
fn canonical_hash_is_sha256_of_canonical_bytes() {
    let value = serde_json::json!({"b": 2, "a": 1});
    let bytes = br#"{"a":1,"b":2}"#;
    assert_eq!(
        canonical_hash(HashKind::Generic, &value).unwrap(),
        Hash::content_sha256(bytes)
    );
    assert_eq!(
        canonical_hash(HashKind::Semantic, &value).unwrap(),
        Hash::semantic_sha256(bytes)
    );
    assert_eq!(
        canonical_hash(HashKind::Evidence, &value).unwrap(),
        Hash::evidence_sha256(bytes)
    );
}
