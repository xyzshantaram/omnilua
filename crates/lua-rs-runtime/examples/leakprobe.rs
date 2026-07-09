//! Leak repro: create+drop N bare Lua VMs. Run under valgrind/heaptrack.
use omnilua::Lua;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    for _ in 0..n {
        let lua = Lua::new();
        drop(lua);
    }
}
