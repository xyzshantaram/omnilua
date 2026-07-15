//! Fast in-memory kit for issue #301 (`io.open`/`os.remove`/`os.rename`
//! errno + message fidelity).
//!
//! The end-to-end oracle (`specs/oracle/diff_one.sh` against the reference
//! binaries) is the truth-teller for the exact triple shape, but it needs a
//! real failing filesystem path and a subprocess per check. This kit installs
//! `HostHooks` that unconditionally fail with a chosen `io::Error` (no real
//! disk I/O, no process spawn) and drives `io.open`/`os.remove`/`os.rename`
//! straight through the SAME mapping path the fix touches: hook return ->
//! `lua-stdlib`'s `file_result`/`os_remove`/`os_rename` -> the Lua failure
//! triple. Milliseconds per test, 100% reproducible.
//!
//! Every expected string/errno here was cross-checked against the
//! version-suffixed reference binaries (`/tmp/lua-refs/bin/lua5.x`) for the
//! same simulated errno, so the assertions pin the reference shape, not our
//! own prior (buggy) output.

use std::cell::Cell;
use std::io;

use omnilua::{HostHooks, Lua, LuaFileHandle, LuaVersion};

thread_local! {
    /// The raw OS errno every installed hook in this file fails with. A
    /// thread-local, not a closure capture, because the hook types are bare
    /// `fn` pointers (mirrors `io_strengthen.rs`'s `SCRATCH_PATH` pattern).
    /// Each test runs on its own thread under the cargo test harness.
    static FAIL_ERRNO: Cell<i32> = const { Cell::new(0) };
}

fn set_fail_errno(code: i32) {
    FAIL_ERRNO.with(|c| c.set(code));
}

fn failing_open(_filename: &[u8], _mode: &[u8]) -> io::Result<Box<dyn LuaFileHandle>> {
    Err(io::Error::from_raw_os_error(FAIL_ERRNO.with(Cell::get)))
}

fn failing_remove(_filename: &[u8]) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(FAIL_ERRNO.with(Cell::get)))
}

fn failing_rename(_from: &[u8], _to: &[u8]) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(FAIL_ERRNO.with(Cell::get)))
}

fn lua_with_failing_fs(version: LuaVersion) -> Lua {
    let hooks = HostHooks::new()
        .file_open(failing_open)
        .file_remove(failing_remove)
        .file_rename(failing_rename);
    Lua::with_hooks_versioned(hooks, version).expect("init lua with failing fs hooks")
}

/// Run `code` under `version` and return the single string it evaluates to.
/// The Lua program itself renders the `(ok, msg, errno)` triple to a string,
/// matching the pattern already established by `io_strengthen.rs::eval_str`.
fn eval_str(version: LuaVersion, code: &str) -> String {
    let lua = lua_with_failing_fs(version);
    lua.load(code)
        .set_name(b"=errno_kit")
        .eval()
        .unwrap_or_else(|e| panic!("eval of `{code}` failed under {version:?}: {e:?}"))
}

const ALL: [LuaVersion; 5] = [
    LuaVersion::V51,
    LuaVersion::V52,
    LuaVersion::V53,
    LuaVersion::V54,
    LuaVersion::V55,
];

const ENOENT: i32 = 2;
const EACCES: i32 = 13;

fn triple_code(call: &str) -> String {
    format!(
        "local ok, msg, errno = {call}; \
         return tostring(ok) .. '|' .. tostring(msg) .. '|' .. tostring(errno)"
    )
}

// ── io.open ──────────────────────────────────────────────────────────────────

#[test]
fn io_open_reports_enoent_errno_and_clean_message() {
    set_fail_errno(ENOENT);
    let code = triple_code("io.open('/nonexistent/x')");
    for v in ALL {
        let got = eval_str(v, &code);
        assert_eq!(
            got, "nil|/nonexistent/x: No such file or directory|2",
            "{v:?}: io.open ENOENT triple should carry the real errno (2) and a clean \
             strerror message, got `{got}`"
        );
    }
}

#[test]
fn io_open_reports_eacces_errno_and_clean_message() {
    set_fail_errno(EACCES);
    let code = triple_code("io.open('/root/protected')");
    for v in ALL {
        let got = eval_str(v, &code);
        assert_eq!(
            got, "nil|/root/protected: Permission denied|13",
            "{v:?}: io.open EACCES triple should carry the real errno (13) and a clean \
             strerror message, got `{got}`"
        );
    }
}

// ── os.remove ────────────────────────────────────────────────────────────────

#[test]
fn os_remove_reports_enoent_errno_and_clean_message() {
    set_fail_errno(ENOENT);
    let code = triple_code("os.remove('/nonexistent/x')");
    for v in ALL {
        let got = eval_str(v, &code);
        assert_eq!(
            got, "nil|/nonexistent/x: No such file or directory|2",
            "{v:?}: os.remove ENOENT triple should carry the real errno (2) and a clean \
             strerror message, got `{got}`"
        );
    }
}

// ── os.rename (version-gated fname prefix) ──────────────────────────────────

/// Verified against every reference binary (5.1-5.5): only Lua 5.1 prefixes
/// the failure message with the source filename; 5.2 onward report bare
/// `strerror` text with no prefix.
#[test]
fn os_rename_reports_enoent_errno_and_version_gated_message() {
    set_fail_errno(ENOENT);
    let code = triple_code("os.rename('/nonexistent/x', 'y')");
    for v in ALL {
        let got = eval_str(v, &code);
        let expected = match v {
            LuaVersion::V51 => "nil|/nonexistent/x: No such file or directory|2",
            _ => "nil|No such file or directory|2",
        };
        assert_eq!(
            got, expected,
            "{v:?}: os.rename ENOENT triple should carry the real errno (2), got `{got}`"
        );
    }
}
