//! Drop-path leak repro: create a Lua, allocate collectable heap objects
//! (post-bootstrap, so they land on the `head` owner list), then drop the Lua.
//! `Heap::drop_all` should free the whole `head` list. Run under valgrind.
use omnilua::Lua;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    for _ in 0..n {
        let lua = Lua::new();
        // Allocate a pile of collectable objects (tables + closures) that are
        // reachable from globals, i.e. on the collectable `head` list.
        lua.load(
            r#"
            local t = {}
            for i = 1, 500 do
                t[i] = { a = i, b = tostring(i), fn = function() return i end }
            end
            _G.__keep = t
        "#,
        )
        .exec()
        .expect("exec");
        drop(lua);
    }
}
