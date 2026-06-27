//! Integration tests for the `serde` feature (`LuaSerdeExt`).
//!
//! Gated on the feature so `cargo test -p omnilua` (no serde) skips the file.

#![cfg(feature = "serde")]

use std::collections::HashMap;

use omnilua::{LossyIntPolicy, Lua, LuaSerdeExt, LuaVersion, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Inner {
    flag: bool,
    ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Outer {
    name: String,
    retries: i64,
    tags: Vec<String>,
    nested: Inner,
    note: Option<String>,
    absent: Option<i64>,
}

fn sample_outer() -> Outer {
    Outer {
        name: "omniLua".to_string(),
        retries: 3,
        tags: vec!["a".to_string(), "b".to_string()],
        nested: Inner { flag: true, ratio: 1.5 },
        note: Some("hello".to_string()),
        absent: None,
    }
}

fn roundtrip<T>(lua: &Lua, value: &T) -> T
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let v = lua.to_value(value).expect("to_value");
    lua.from_value(v).expect("from_value")
}

#[test]
fn struct_with_vec_option_and_nesting_roundtrips() {
    let lua = Lua::new();
    let original = sample_outer();
    let back: Outer = roundtrip(&lua, &original);
    assert_eq!(original, back);
}

#[test]
fn none_field_is_omitted_then_defaults_back_to_none() {
    let lua = Lua::new();
    let original = sample_outer();
    let value = lua.to_value(&original).expect("to_value");
    let table = match &value {
        Value::Table(t) => t.clone(),
        other => panic!("expected table, got {other:?}"),
    };
    let absent: Value = table.get("absent").expect("get absent");
    assert!(matches!(absent, Value::Nil), "None field must be nil/absent");
    let back: Outer = lua.from_value(value).expect("from_value");
    assert_eq!(back.absent, None);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Shape {
    Empty,
    Circle(f64),
    Rect(f64, f64),
    Named { label: String, sides: i64 },
}

#[test]
fn externally_tagged_enum_all_variant_kinds_roundtrip() {
    let lua = Lua::new();
    for shape in [
        Shape::Empty,
        Shape::Circle(2.5),
        Shape::Rect(3.0, 4.0),
        Shape::Named { label: "square".to_string(), sides: 4 },
    ] {
        let back: Shape = roundtrip(&lua, &shape);
        assert_eq!(shape, back);
    }
}

#[test]
fn unit_variant_serializes_to_a_string() {
    let lua = Lua::new();
    let v = lua.to_value(&Shape::Empty).expect("to_value");
    match v {
        Value::String(s) => assert_eq!(s.to_str().unwrap(), "Empty"),
        other => panic!("unit variant should be a string, got {other:?}"),
    }
}

#[test]
fn map_and_vec_roundtrip() {
    let lua = Lua::new();
    let mut map: HashMap<String, i64> = HashMap::new();
    map.insert("one".to_string(), 1);
    map.insert("two".to_string(), 2);
    let back: HashMap<String, i64> = roundtrip(&lua, &map);
    assert_eq!(map, back);

    let seq = vec![10i64, 20, 30];
    let back_seq: Vec<i64> = roundtrip(&lua, &seq);
    assert_eq!(seq, back_seq);
}

#[test]
fn tuple_roundtrips_as_array() {
    let lua = Lua::new();
    let tuple = (1i64, "two".to_string(), true);
    let back: (i64, String, bool) = roundtrip(&lua, &tuple);
    assert_eq!(tuple, back);
}

#[test]
fn scalars_roundtrip() {
    let lua = Lua::new();
    assert_eq!(roundtrip(&lua, &42i64), 42i64);
    assert_eq!(roundtrip(&lua, &true), true);
    assert_eq!(roundtrip(&lua, &"text".to_string()), "text".to_string());
    let f: f64 = roundtrip(&lua, &3.25f64);
    assert_eq!(f, 3.25);
}

#[test]
fn arbitrary_bytes_roundtrip_through_lua_strings() {
    let lua = Lua::new();
    let raw = serde_bytes::ByteBuf::from(vec![0u8, 159, 146, 150, 255]);
    let value = lua.to_value(&raw).expect("to_value");
    assert!(matches!(value, Value::String(_)), "bytes must become a Lua string");
    let back: serde_bytes::ByteBuf = lua.from_value(value).expect("from_value");
    assert_eq!(raw, back);
}

#[test]
fn serde_json_value_roundtrips() {
    let lua = Lua::new();
    let json = serde_json::json!({
        "name": "valdr",
        "count": 7,
        "ratio": 0.5,
        "enabled": true,
        "tags": ["x", "y", "z"],
        "nested": { "k": 1 }
    });
    let value = lua.to_value(&json).expect("to_value");
    let back: serde_json::Value = lua.from_value(value).expect("from_value");
    assert_eq!(json, back);
}

#[test]
fn serialized_value_is_real_lua_data() {
    let lua = Lua::new();
    let cfg = sample_outer();
    let value = lua.to_value(&cfg).expect("to_value");
    lua.globals().set("config", value).expect("set global");
    let name: String = lua.load("return config.name").eval().expect("eval name");
    assert_eq!(name, "omniLua");
    let first_tag: String = lua.load("return config.tags[1]").eval().expect("eval tag");
    assert_eq!(first_tag, "a");
    let ratio: f64 = lua.load("return config.nested.ratio").eval().expect("eval ratio");
    assert_eq!(ratio, 1.5);
}

#[test]
fn integer_crosses_the_number_model_seam() {
    let inexact = (1i64 << 53) + 1;

    let lua54 = Lua::new();
    assert!(
        matches!(lua54.to_value(&inexact).unwrap(), Value::Integer(v) if v == inexact),
        "5.4 (dual number model) keeps the host integer an integer"
    );

    let lua51 = Lua::new_versioned(LuaVersion::V51);
    assert!(
        matches!(lua51.to_value(&inexact).unwrap(), Value::Number(_)),
        "5.1 (float-only) widens to a float under the default WidenLossy policy"
    );

    lua51.set_lossy_int_policy(LossyIntPolicy::ErrorOnInexact);
    assert!(
        lua51.to_value(&inexact).is_err(),
        "ErrorOnInexact rejects an integer with no exact float representation"
    );
    assert!(
        matches!(lua51.to_value(&5i64).unwrap(), Value::Number(n) if n == 5.0),
        "an exactly representable integer is still accepted under ErrorOnInexact"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// PORT STATUS
//   source:        (no C analog — serde integration tests)
//   target_crate:  omnilua
//   confidence:    high
//   todos:         0
//   port_notes:    0
//   unsafe_blocks: 0
//   notes:         Exercises LuaSerdeExt round-trips (structs, enums, maps,
//                  tuples, bytes, serde_json interop), the None-field omission
//                  rule, real-Lua-data check, and the number-model seam.
// ──────────────────────────────────────────────────────────────────────────
