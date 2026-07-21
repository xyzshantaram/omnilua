//! The CLI's `os.execute`/`io.popen` hooks must route through the platform
//! shell — `/bin/sh -c` on POSIX, `%COMSPEC% /C` on Windows — like C's
//! `system(3)`/`_popen`. These scenarios use only commands both shells
//! accept (`exit <n>`, `echo`), so one expectation holds everywhere; the
//! Windows-only `\r` that `cmd.exe`'s `echo` emits is stripped in-script
//! (real C Lua opens the popen stream in text mode, where the CRT strips it;
//! see docs/WINDOWS_DIVERGENCES.md).

use std::process::Command;

fn run(code: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_omnilua"))
        .arg("-e")
        .arg(code)
        .output()
        .expect("spawn omnilua");
    assert!(
        out.status.success(),
        "omnilua exited nonzero; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `luaL_execresult` on 5.4: nonzero exit is the `fail` (nil) triple.
#[test]
fn os_execute_reports_exit_code_via_shell() {
    let got = run("local ok, how, code = os.execute('exit 7') print(tostring(ok), how, code)");
    assert_eq!(got.trim_end(), "nil\texit\t7");
}

#[test]
fn os_execute_zero_exit_is_true() {
    let got = run("local ok, how, code = os.execute('exit 0') print(tostring(ok), how, code)");
    assert_eq!(got.trim_end(), "true\texit\t0");
}

#[test]
fn io_popen_reads_shell_output() {
    let got = run(
        "local f = assert(io.popen('echo hi')) \
         local s = f:read('l') f:close() print((s:gsub('\\r$','')))",
    );
    assert_eq!(got.trim_end(), "hi");
}
