//! RFC 8785 (JCS) canonical JSON through `serde_json_canonicalizer`, and SHA-256 over
//! canonical bytes.
//!
//! This primitive canonicalizes exactly the value supplied to it. Plumb domain-specific
//! ordering of semantically unordered arrays is applied by semantic-envelope construction
//! before calling it (plan §6.3); it is not done here.
//!
//! Before serializing, the value is walked once by a validating serializer that rejects
//! everything the canonical bytes could not represent faithfully (Fable #27/#28): a duplicate
//! member name (including a stringified map key), an integer or integer map key outside the
//! I-JSON range ±(2^53−1), a non-finite float at any depth, a raw JSON fragment and a map key
//! that is neither a string nor an integer.

use std::collections::BTreeSet;
use std::fmt;

use serde::ser::{self, Impossible, Serialize};

use crate::error::CoreError;
use crate::hash::{Hash, HashKind};

/// Serializes `value` to RFC 8785 canonical JSON bytes using `serde_json_canonicalizer`.
pub fn to_canonical_json<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CoreError> {
    value
        .serialize(Validator)
        .map_err(|e| CoreError::Canonicalization(e.0))?;
    serde_json_canonicalizer::to_vec(&value).map_err(|e| CoreError::Canonicalization(e.to_string()))
}

/// SHA-256 over the canonical JSON bytes of `value`, as a hash of the given kind.
pub fn canonical_hash<T: Serialize + ?Sized>(kind: HashKind, value: &T) -> Result<Hash, CoreError> {
    let bytes = to_canonical_json(value)?;
    Ok(match kind {
        HashKind::Generic => Hash::content_sha256(&bytes),
        HashKind::Semantic => Hash::semantic_sha256(&bytes),
        HashKind::Evidence => Hash::evidence_sha256(&bytes),
    })
}

/// 2^53 − 1: the largest magnitude of an integer that every I-JSON consumer represents exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// The `serde_json` private struct name a `RawValue` serializes under.
const RAW_VALUE_TOKEN: &str = "$serde_json::private::RawValue";

#[derive(Debug)]
struct Invalid(String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

impl ser::Error for Invalid {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Invalid(msg.to_string())
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, Invalid> {
    Err(Invalid(message.into()))
}

fn check_unsigned(value: u128) -> Result<(), Invalid> {
    if value > u128::from(MAX_SAFE_INTEGER) {
        return invalid(format!("integer {value} is outside the I-JSON range ±(2^53−1)"));
    }
    Ok(())
}

fn check_signed(value: i128) -> Result<(), Invalid> {
    if value.unsigned_abs() > u128::from(MAX_SAFE_INTEGER) {
        return invalid(format!("integer {value} is outside the I-JSON range ±(2^53−1)"));
    }
    Ok(())
}

fn check_float(value: f64) -> Result<(), Invalid> {
    if !value.is_finite() {
        return invalid("NaN and ±Infinity are not representable in JSON");
    }
    Ok(())
}

/// A serializer that produces nothing and fails on the first value canonical JSON cannot
/// represent faithfully.
struct Validator;

/// Member names seen so far in one JSON object.
#[derive(Default)]
struct Members(BTreeSet<String>);

impl Members {
    fn insert(&mut self, name: String) -> Result<(), Invalid> {
        if !self.0.insert(name.clone()) {
            return invalid(format!("duplicate member name {name:?}"));
        }
        Ok(())
    }
}

impl ser::Serializer for Validator {
    type Ok = ();
    type Error = Invalid;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = MapValidator;
    type SerializeStruct = StructValidator;
    type SerializeStructVariant = StructValidator;

    fn serialize_bool(self, _: bool) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_i64(self, value: i64) -> Result<(), Invalid> {
        check_signed(value.into())
    }
    fn serialize_i128(self, value: i128) -> Result<(), Invalid> {
        check_signed(value)
    }
    fn serialize_u8(self, _: u8) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_u64(self, value: u64) -> Result<(), Invalid> {
        check_unsigned(value.into())
    }
    fn serialize_u128(self, value: u128) -> Result<(), Invalid> {
        check_unsigned(value)
    }
    fn serialize_f32(self, value: f32) -> Result<(), Invalid> {
        check_float(value.into())
    }
    fn serialize_f64(self, value: f64) -> Result<(), Invalid> {
        check_float(value)
    }
    fn serialize_char(self, _: char) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_str(self, _: &str) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Invalid> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Invalid> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Invalid> {
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<MapValidator, Invalid> {
        Ok(MapValidator::default())
    }
    fn serialize_struct(self, name: &'static str, _: usize) -> Result<StructValidator, Invalid> {
        if name == RAW_VALUE_TOKEN {
            return invalid("raw JSON values cannot be canonicalized");
        }
        Ok(StructValidator::default())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<StructValidator, Invalid> {
        Ok(StructValidator::default())
    }
}

impl ser::SerializeSeq for Validator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Invalid> {
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

impl ser::SerializeTuple for Validator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Invalid> {
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for Validator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Invalid> {
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for Validator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Invalid> {
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

/// Validates one map: every key must be a string or an in-range integer, and no two keys may
/// produce the same member name.
#[derive(Default)]
struct MapValidator(Members);

impl ser::SerializeMap for MapValidator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Invalid> {
        self.0.insert(key.serialize(KeyName)?)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Invalid> {
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

/// Validates one struct (or struct variant): its field names must be distinct.
#[derive(Default)]
struct StructValidator(Members);

impl ser::SerializeStruct for StructValidator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Invalid> {
        self.0.insert(name.to_owned())?;
        value.serialize(Validator)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for StructValidator {
    type Ok = ();
    type Error = Invalid;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Invalid> {
        ser::SerializeStruct::serialize_field(self, name, value)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}

/// Produces the JSON member name a map key serializes to, exactly as `serde_json` stringifies
/// it; anything that is not a string or an in-range integer is rejected.
struct KeyName;

fn non_string_key<T>() -> Result<T, Invalid> {
    invalid("map keys must be strings or integers")
}

impl ser::Serializer for KeyName {
    type Ok = String;
    type Error = Invalid;
    type SerializeSeq = Impossible<String, Invalid>;
    type SerializeTuple = Impossible<String, Invalid>;
    type SerializeTupleStruct = Impossible<String, Invalid>;
    type SerializeTupleVariant = Impossible<String, Invalid>;
    type SerializeMap = Impossible<String, Invalid>;
    type SerializeStruct = Impossible<String, Invalid>;
    type SerializeStructVariant = Impossible<String, Invalid>;

    fn serialize_bool(self, _: bool) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_i8(self, value: i8) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_i16(self, value: i16) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_i32(self, value: i32) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_i64(self, value: i64) -> Result<String, Invalid> {
        check_signed(value.into())?;
        Ok(value.to_string())
    }
    fn serialize_i128(self, value: i128) -> Result<String, Invalid> {
        check_signed(value)?;
        Ok(value.to_string())
    }
    fn serialize_u8(self, value: u8) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_u16(self, value: u16) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_u32(self, value: u32) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_u64(self, value: u64) -> Result<String, Invalid> {
        check_unsigned(value.into())?;
        Ok(value.to_string())
    }
    fn serialize_u128(self, value: u128) -> Result<String, Invalid> {
        check_unsigned(value)?;
        Ok(value.to_string())
    }
    fn serialize_f32(self, _: f32) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_f64(self, _: f64) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_char(self, value: char) -> Result<String, Invalid> {
        Ok(value.to_string())
    }
    fn serialize_str(self, value: &str) -> Result<String, Invalid> {
        Ok(value.to_owned())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_none(self) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_unit(self) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Invalid> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, Invalid> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, Invalid> {
        non_string_key()
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Invalid> {
        non_string_key()
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Invalid> {
        non_string_key()
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Invalid> {
        non_string_key()
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Invalid> {
        non_string_key()
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Invalid> {
        non_string_key()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Invalid> {
        non_string_key()
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Invalid> {
        non_string_key()
    }
}
