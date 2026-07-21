# Windows divergences ledger

Known and suspected behavior differences between omniLua on Windows and real
C Lua built with MSVC. This exists because the oracle has never spoken on
Windows: the reference binaries and diff scripts are POSIX-only, so Windows
fidelity claims are exactly as strong as the entries below and no stronger.

What executes on Windows today:

- the release gate (`release.yml` `windows-build`) builds AND runs the full
  workspace test suite on `windows-latest` MSVC before anything publishes;
- `windows-check.yml` (`workflow_dispatch`) runs the same on any branch on
  demand (`gh workflow run windows-check.yml --ref <branch>`);
- reference-binary oracle tests self-skip there (`/tmp/lua-refs` is absent),
  so what runs is the baked-constant oracle plus all unit/integration tests.

The end state this ledger works toward: build the pinned reference Lua with
MSVC on a Windows runner and run the snippet/official oracles there. Until
then, every entry is either FIXED (mechanism ported, covered by tests that
run on the Windows gate), OPEN (known divergence, decision pending), or
UNVERIFIED (suspected, never measured against a Windows reference build).

## FIXED

- **`localtime_s`/`gmtime_s` link failure** (#308, PR #309): the CLI's
  Windows local-offset hook now calls the real MSVCRT exports
  `_localtime64_s`/`_gmtime64_s`; the unprefixed names are CRT header-inline
  wrappers that newer toolchains (VS 18 / MSVC 14.5x, including today's
  `windows-latest` image) no longer resolve at link time.
- **`os.execute`/`io.popen` shell** (PR #313): the hooks hardcoded
  `/bin/sh -c`, so every command failed to spawn on Windows. They now run
  `%COMSPEC% /C` with the command text passed raw (the CRT `system`/`_popen`
  contract; `cmd.exe` when `COMSPEC` is unset).
- **`package.path`/`package.cpath` defaults + `LUA_EXEC_DIR`** (PR #311):
  the Windows defaults were version-flat stubs; they are now byte-faithful
  per-version transcriptions of each release's `luaconf.h` `_WIN32` branch,
  and `setprogdir` replaces every `!` with the executable's directory —
  after the `;;` splice and on env-provided values too, matching C's
  ordering.
- **local-offset day math** (PR #313): the `tm`-differencing behind `os.date`
  offsets (year-boundary pinning, sub-hour zones) is a pure function
  unit-tested on every platform, not Windows-only dead weight.

## OPEN

- **Text-mode stdio.** C Lua opens files (and popen streams) through `fopen`
  defaults, which on Windows means TEXT mode: CRLF→LF on read, LF→CRLF on
  write, Ctrl-Z as EOF. Rust I/O is always binary, so `io.read("l")` on a
  CRLF file keeps the `\r` where C strips it, and written `"\n"` stays LF
  where C emits CRLF. This needs a decision: emulate CRT text mode in our io
  layer under Windows, or document binary-always as intentional. The
  `shell_hooks` popen test strips `\r` in-script for exactly this reason.

## UNVERIFIED (never measured against a Windows reference build)

- **`os.date`/`os.time` format surface.** The offset mechanism is ported,
  but C's `strftime` output (`%c`, `%x`, locale month/day names) comes from
  the CRT locale; ours comes from our own formatter. Divergence likely for
  locale-sensitive formats.
- **`os.tmpname`/`io.tmpfile` shapes.** C uses `tmpnam`/`tmpfile` (MSVC:
  root-relative names; `tmpfile` may need elevated rights); our hooks build
  unique names under `std::env::temp_dir()`. Functionally fine, byte shapes
  differ.
- **Filename encoding.** C Lua passes filename bytes to the ANSI (`*A`) CRT
  APIs; Rust converts through UTF-16, so byte strings that are not valid
  UTF-8 behave differently as paths (and the `setprogdir` executable
  directory goes through `to_string_lossy`).
- **`package.loadlib` error text.** C reports `FormatMessageA` strings;
  ours reports `libloading`'s. Message wording will differ on failure paths.
- **`lua-rs-lfs` on Windows.** Symlink creation needs a privilege or
  Developer Mode; attribute fields (`dev`/`ino`/`uid`) carry different
  semantics than POSIX.
- **`os.clock`.** MSVC's `clock()` is wall time since process start — which
  our CLI hook also is — so on Windows the hook is coincidentally MORE
  faithful than on POSIX (where C's `clock()` is CPU time). Listed to record
  the asymmetry, not to fix it here.
