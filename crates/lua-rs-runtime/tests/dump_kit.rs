//! `dump_kit` — golden + roundtrip kit for the `string.dump` bytecode header
//! across all five versions (the deferred per-version-header architectural item).
//!
//! Flavor: golden constants (the reference header bytes, captured once into
//! `tests/golden/dump_headers.tsv` by `harness/gen_golden.sh`) + a roundtrip
//! invariant (`dump -> load -> call` must reproduce the value). Pure in-process:
//! no reference binary, no subprocess, no `/tmp` dependency — the rung-2 inner
//! loop the dump-header fix develops against, in milliseconds.
//!
//! COVERED: the header bytes a freshly-dumped function emits per version
//! (signature, version tag, format, size fields, sentinels) — exactly the prefix
//! `calls.lua` asserts — and that our own `dump`/`undump` round-trip.
//! NOT COVERED: full cross-binary BODY byte-fidelity vs the reference C dumper
//! (5.1/5.2 use a structurally different body our internal format does not emit);
//! the kit pins the header contract and self-consistency, not body interchange.

use omnilua::{Lua, LuaVersion};

const VERSIONS: &[(&str, LuaVersion)] = &[
    ("5.1", LuaVersion::V51),
    ("5.2", LuaVersion::V52),
    ("5.3", LuaVersion::V53),
    ("5.4", LuaVersion::V54),
    ("5.5", LuaVersion::V55),
];

/// Evaluate `code` under `version`, returning the string it produces. The
/// snippet is `load`/`loadstring`+`pcall`ed inside Lua so the running version's
/// own loader and renderer are exercised.
fn eval_str(version: LuaVersion, code: &str) -> String {
    let lua = Lua::new_versioned(version);
    let loader = if version == LuaVersion::V51 {
        "loadstring"
    } else {
        "load"
    };
    let wrapper = format!(
        "local f, e = {loader}([==[\n{code}\n]==])\n\
         if not f then error('load: ' .. tostring(e)) end\n\
         return f()"
    );
    lua.load(&wrapper)
        .eval()
        .unwrap_or_else(|e| panic!("dump_kit harness failure: {e:?}"))
}

/// Hex of the first `n` bytes of `string.dump(function() return 1 end)` under
/// `version`, computed inside Lua (avoids binary-in-`String` round-tripping).
fn our_header_hex(version: LuaVersion, n: usize) -> String {
    let code = format!(
        "local d = string.dump(function() return 1 end)\n\
         local t = {{}}\n\
         for i = 1, {n} do t[i] = string.format('%02x', string.byte(d, i)) end\n\
         return table.concat(t)"
    );
    eval_str(version, &code)
}

/// Parse the committed golden: version -> (header_len, hex_bytes).
fn golden() -> Vec<(String, usize, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/dump_headers.tsv");
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read golden {path}: {e} (run harness/gen_golden.sh)"));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let v = it.next().unwrap().to_string();
            let n: usize = it.next().unwrap().parse().unwrap();
            let hex = it.next().unwrap().to_string();
            (v, n, hex)
        })
        .collect()
}

fn version_of(tag: &str) -> LuaVersion {
    VERSIONS
        .iter()
        .find(|(s, _)| *s == tag)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("unknown version tag {tag}"))
}

#[test]
fn dump_header_matches_reference_golden() {
    let mut failures = Vec::new();
    for (tag, n, want_hex) in golden() {
        let version = version_of(&tag);
        let got_hex = our_header_hex(version, n);
        if got_hex != want_hex {
            failures.push(format!(
                "  {tag}: header mismatch (first {n} bytes)\n      ours: {got_hex}\n      ref : {want_hex}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "string.dump header diverges from reference:\n{}",
        failures.join("\n")
    );
}

#[test]
fn dump_load_roundtrip_reproduces_value() {
    for (tag, version) in VERSIONS {
        let got = eval_str(
            *version,
            "local d = string.dump(function() return 42 end)\n\
             local f = (loadstring or load)(d)\n\
             return tostring(f())",
        );
        assert_eq!(got, "42", "dump->load roundtrip failed on {tag}");
    }
}

/// Round-trips a chunk whose top proto owns nested function prototypes, so the
/// undumper's `load_protos` sub-proto construction path (now routed through
/// `mark_gc_check_needed` + build-then-wrap, issue #276) is exercised: the
/// reloaded chunk must rebuild the whole proto tree and run it.
#[test]
fn dump_load_roundtrip_nested_functions() {
    for (tag, version) in VERSIONS {
        let got = eval_str(
            *version,
            "local d = string.dump(function()\n\
             \x20 local function add(a, b) return a + b end\n\
             \x20 local function mul(a, b) return a * b end\n\
             \x20 return add(mul(2, 3), 4)\n\
             end)\n\
             local f = (loadstring or load)(d)\n\
             return tostring(f())",
        );
        assert_eq!(
            got, "10",
            "nested-function dump->load roundtrip failed on {tag}"
        );
    }
}

/// Round-trips a chunk that, once reloaded and run, builds a nested closure
/// capturing a live upvalue and drives it across several calls. This exercises
/// both the reloaded top closure (built via `new_lclosure`, which fills its
/// upvalue slots) and the runtime upvalue machinery over the reloaded proto
/// tree (issue #276).
#[test]
fn dump_load_roundtrip_closure_upvalues() {
    for (tag, version) in VERSIONS {
        let got = eval_str(
            *version,
            "local d = string.dump(function()\n\
             \x20 local counter = 0\n\
             \x20 return function() counter = counter + 1; return counter end\n\
             end)\n\
             local make = (loadstring or load)(d)\n\
             local c = make()\n\
             c(); c()\n\
             return tostring(c())",
        );
        assert_eq!(
            got, "3",
            "closure-upvalue dump->load roundtrip failed on {tag}"
        );
    }
}

/// Loads many precompiled chunks back-to-back and then forces a collection.
/// Each load now marks the collector (`new_lclosure` -> `mark_gc_check_needed`),
/// so this stresses the mark-on-chunk-load path under repeated loads and
/// confirms a subsequent `collectgarbage()` leaves the results intact rather
/// than sweeping the freshly reloaded closures (issue #276). A deterministic
/// "collection was deferred" assertion is not feasible from Lua, so this covers
/// the path via bulk load + collect consistency instead.
#[test]
fn many_loads_then_gc_stays_consistent() {
    for (tag, version) in VERSIONS {
        let got = eval_str(
            *version,
            "local sum = 0\n\
             for _ = 1, 200 do\n\
             \x20 local d = string.dump(function() return 7 end)\n\
             \x20 local f = (loadstring or load)(d)\n\
             \x20 sum = sum + f()\n\
             \x20 collectgarbage('step')\n\
             end\n\
             collectgarbage()\n\
             return tostring(sum)",
        );
        assert_eq!(got, "1400", "many-loads+gc consistency failed on {tag}");
    }
}
