# Smoke testing every command

Every user-visible behaviour in CADCraft is a command, so a sweep over the whole registry is the
cheapest broad check we have: it finds panics, prompt machines that never terminate, and commands
that break when run with no parameters. The sweep is scripted, runs headless, and each run is
recorded with what it cost to produce.

```sh
cargo xtask smoke                      # sweep + record the run and its cost
cargo xtask smoke -- --filter dim      # only commands whose id contains "dim"
cadcraft-cli smoke --out /tmp/sweep    # the runner on its own, no ledger
cargo xtask metrics --since -4h        # agent time, tokens and models, any window
```

## What the sweep does

`cadcraft-cli smoke` enumerates `cadcraft_engine::cmd::command_specs()` and gives every command a
fresh session over the sample drawing, with everything selected, in two phases:

| Phase | What it exercises |
|---|---|
| `json` | `Session::execute(id, null)` — the programmatic form every agent, script and CLI call takes. Never opens a dialog. |
| `interactive` | `Session::start(id)`, then the prompt machine is driven generically from `current_prompt()`: points, numbers, text, picks and Enter, per what the prompt accepts. |

Each (command, phase) lands in `results.jsonl` with one of four statuses:

- **pass** — ran clean, or the prompt machine finished.
- **expected_error** — `BadParams` or `Disabled`: the command legitimately needs parameters,
  a selection or state the sweep did not supply. Not a failure.
- **fail** — a caught panic (`Internal`), a prompt that never terminates, a timeout overrun, or a
  consistency check tripping. **These are the bugs the sweep exists to find.**
- **skip** — deliberately excluded, with the reason in the record (e.g. commands that would quit
  the process).

The sweep does **not** yet cover the UI path — menus, toolbars and the mouse. The result schema
reserves `transport` (`inproc` today, `control` later) for a second transport that drives the
running app over [the control protocol](control-protocol.md).

## Artifacts

One directory per run, `target/smoke/<run_id>/`:

| File | Contents |
|---|---|
| `results.jsonl` | one record per (command, phase): status, duration, error, entity counts, inputs fed |
| `summary.json` | totals, per-phase rollup, every failure, the 20 slowest commands |
| `report.md` | the same, readable: headline counts, failures table, per-menu rollup |
| `metrics.json` | the ledger entry for this run: sweep totals plus the agent metrics below |

Runs live under `target/` and are not committed. The history is, in `smoke/ledger.jsonl`
(append-only, one JSON object per run) and `smoke/LEDGER.md` (generated from it, newest first).

## Run metrics: time, tokens, models

CADCraft is largely built by agents, so a run is recorded with what it cost. `cargo xtask smoke`
attributes the window from the end of the previous ledger entry to the end of this sweep (override
with `--since`), and `cargo xtask metrics` reports any window on its own:

- **time** — the sweep's own wall-clock, and the agent wall-clock across the window
- **tokens** — input, output, cache creation, cache reads, thinking, split per model and per agent
  (`main` versus `subagent`, so parallel workers are visible)
- **models** — which model ids and effort levels spent them
- **tool calls** — how many, and the busiest tools

The numbers come from the local Claude Code session transcripts
(`~/.claude/projects/<slug>/*.jsonl`), which record `message.model` and `message.usage` per reply.
A reply is written as several lines sharing `message.id`, so usage is counted once per message id
and tool calls once per `tool_use` id. Only counters are read — no transcript content is copied
anywhere, and nothing leaves the machine. With no transcripts present the sweep still runs and the
ledger row simply shows `—`.

`--no-metrics` skips that step entirely.
