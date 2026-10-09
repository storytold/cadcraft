# Smoke-test ledger

One row per `cargo xtask smoke` run, newest first. Generated — do not edit by hand.

`sweep` is how long the command sweep took; `agent` columns are the Claude Code work that led to the run
(wall-clock between the first and last agent message in the window, tokens including cache reads, and the
models that spent them). See `docs/smoke-tests.md`.

| run | commit | tested | pass | exp-err | fail | skip | sweep | agent time | agent tokens | models |
|---|---|---|---|---|---|---|---|---|---|---|
| 20261009-160948 | `86f3199` | 291 | 249 | 175 | 0 | 4 | 1.6s | 51m 26s | 20,556,692 | claude-opus-5 (high) |
