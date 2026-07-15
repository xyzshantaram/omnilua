//! Handle identity & equality — issue #225.
//!
//! `to_pointer` exposes the VM's object identity (the same mapping
//! `lua_topointer` uses) on the high-level handles, and `PartialEq` compares
//! reference handles by identity and strings by bytes. These are the primitives
//! a host needs to walk a table graph safely (dedup, cycle detection, the
//! cross-version marshalling bridge).

use omnilua::{Function, Lua, LuaString, LuaVersion, Table, Value};
use std::collections::HashSet;

#[test]
fn same_table_shares_identity_and_compares_equal() {
    let lua = Lua::new();
    let t = lua.create_table().unwrap();
    lua.globals().set("t", t).unwrap();

    let a: Table = lua.globals().get("t").unwrap();
    let b: Table = lua.globals().get("t").unwrap();

    assert_eq!(a.to_pointer().unwrap(), b.to_pointer().unwrap());
    assert_eq!(a, b);
}

#[test]
fn distinct_tables_differ() {
    let lua = Lua::new();
    let t1 = lua.create_table().unwrap();
    let t2 = lua.create_table().unwrap();

    assert_ne!(t1.to_pointer().unwrap(), t2.to_pointer().unwrap());
    assert_ne!(t1, t2);
}

#[test]
fn function_identity_round_trips() {
    let lua = Lua::new();
    let f: Function = lua.load("return function() return 1 end").eval().unwrap();
    lua.globals().set("f", f.clone()).unwrap();
    let g: Function = lua.globals().get("f").unwrap();

    assert_eq!(f, g);
    assert_eq!(f.to_pointer().unwrap(), g.to_pointer().unwrap());
}

#[test]
fn string_equality_is_by_bytes() {
    let lua = Lua::new();
    let s1: LuaString = lua.create_string("hello").unwrap();
    let s2: LuaString = lua.create_string("hello").unwrap();
    let s3: LuaString = lua.create_string("world").unwrap();

    assert_eq!(s1, s2);
    assert_ne!(s1, s3);
    s1.to_pointer().unwrap();
}

#[test]
fn scalar_values_have_no_identity() {
    assert_eq!(Value::Nil.to_pointer().unwrap(), None);
    assert_eq!(Value::Boolean(true).to_pointer().unwrap(), None);
    assert_eq!(Value::Integer(3).to_pointer().unwrap(), None);
    assert_eq!(Value::Number(2.5).to_pointer().unwrap(), None);
}

#[test]
fn cyclic_table_walk_terminates_with_identity_visited_set() {
    let lua = Lua::new();
    let t = lua.create_table().unwrap();
    t.raw_set("self", t.clone()).unwrap();
    t.raw_set("n", 1).unwrap();

    fn count(t: &Table, seen: &mut HashSet<usize>) -> usize {
        if !seen.insert(t.to_pointer().unwrap()) {
            return 0;
        }
        let mut n = 1;
        for (_k, v) in t.raw_pairs().unwrap() {
            if let Value::Table(child) = v {
                n += count(&child, seen);
            }
        }
        n
    }

    let mut seen = HashSet::new();
    assert_eq!(count(&t, &mut seen), 1);
}

#[test]
fn identity_is_version_invariant() {
    for v in [LuaVersion::V51, LuaVersion::V54] {
        let lua = Lua::new_versioned(v);
        let t = lua.create_table().unwrap();
        lua.globals().set("t", t).unwrap();

        let a: Table = lua.globals().get("t").unwrap();
        let b: Table = lua.globals().get("t").unwrap();
        assert_eq!(a, b, "self-identity must hold on {v:?}");

        let other = lua.create_table().unwrap();
        assert_ne!(a, other, "distinct tables must differ on {v:?}");
    }
}

/// Issue #278 fix-round-2: a light C function's public `to_pointer()` must be
/// its real code address — the same value the VM's own `%p` and `tostring`
/// print — never the tiny `c_functions` registry index. `print` is a stdlib
/// LightC function. Guards against the embedding handle diverging from the VM's
/// pointer resolver.
#[test]
fn lightc_function_pointer_is_real_address_not_registry_index() {
    let lua = Lua::new();
    let print_fn: Function = lua.globals().get("print").unwrap();
    let ptr = print_fn.to_pointer().unwrap();
    assert!(
        ptr > u16::MAX as usize,
        "handle must expose a real address, got {ptr:#x} (looks like a registry index)"
    );

    let via_p: String = lua
        .load("return string.format('%p', print)")
        .eval()
        .unwrap();
    let p_ptr = usize::from_str_radix(via_p.trim_start_matches("0x"), 16).unwrap();
    assert_eq!(ptr, p_ptr, "handle pointer must equal the VM %p address");

    let via_tostring: String = lua.load("return tostring(print)").eval().unwrap();
    assert!(
        via_tostring.starts_with("function: 0x"),
        "unexpected tostring form: {via_tostring}"
    );
    let ts_ptr = usize::from_str_radix(via_tostring.rsplit_once("0x").unwrap().1, 16).unwrap();
    assert_eq!(ts_ptr, ptr, "tostring address must equal the handle address");

    let type_fn: Function = lua.globals().get("type").unwrap();
    assert_ne!(
        ptr,
        type_fn.to_pointer().unwrap(),
        "distinct stdlib functions must have distinct pointers"
    );
    let print_again: Function = lua.globals().get("print").unwrap();
    assert_eq!(
        ptr,
        print_again.to_pointer().unwrap(),
        "the same function must have a stable pointer"
    );
}
