#!/usr/bin/env bash
# RETIRED 2026-07-13: PORT STATUS trailers were removed from crates/**/*.rs
# when the repo moved beyond port status (the de-port sweep). This hook is
# kept as a no-op so chassis callers (commit-on-stop.sh, harness/stop-hook.sh,
# port-harness copies) keep working without wiring changes elsewhere.
exit 0
