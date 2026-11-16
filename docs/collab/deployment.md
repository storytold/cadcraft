# CADCraft collaboration: deployment

How `cadcraft-collab-server` runs, what it expects from the cabinet and what it
guarantees. The protocol and its decisions are in `rfc.md` (§1–§14); the ADRs
(0001–0014) were accepted in the cabinet's protocol track and their content is
carried by those sections — this repository does not contain `.craft/`.

This file describes the target contract. Where the current binary is weaker
(missing flags, claim re-validation, state durability, dedup persistence), the
gap is named in place rather than papered over.

## One process, one drawing

One process serves one drawing (`--file`). A room is authoritative for that
drawing: it applies every operation, orders them, and answers with the change
set. Several drawings are several processes; a crashing room costs that drawing
only. The limit is deliberate: a 200k-entity `Drawing` is not a shared OOM
domain.

```bash
cadcraft-collab-server \
  --listen 127.0.0.1:8787 \
  --file /srv/office/drawings/plan.dxf \
  --session 6f1c0b7a-2d44-4e91-8a3c-5b6e9f0d1c72 \
  --secret-file /run/secrets/office-sync-hs256 \
  --issuer cabinet --audience office-sync \
  --state /var/lib/office-sync/6f1c0b7a.dxf \
  --snapshot-secs 300 \
  --http 127.0.0.1:8788
```

`--dev-token <token>:<role>` (repeatable) replaces `--secret-file` for tests and
a single-tenant deployment; it is never used where the cabinet signs tokens.
Prefer the environment or a file over argv: an argument is visible in the
process list.

Flags the binary has today (measured on the `feature/realtime-collaboration-server`
head; defaults are the code's, not the policy's):

| flag | default | meaning |
|---|---|---|
| `--listen` | `127.0.0.1:8787` | WebSocket listener |
| `--file` | — (required) | source drawing |
| `--session` | `session` | session id; must match the JWT `sessionId` |
| `--secret-file` | — | file with the HS256 secret |
| `--issuer` / `--audience` | — | required `iss` / `aud` |
| `--dev-token` | — | fixed `token:role`, repeatable (tests only) |
| `--state` | — | snapshot payload path; journal and version derive from it |
| `--assets` | — | attachment store root (ADR-0021) |
| `--cabinet-url` | — | cabinet commit endpoint; with it, `--file-id`, `--cabinet-token-file`, `--cabinet-timeout-ms`, `--cabinet-attempts` |
| `--snapshot-secs` | `30` | seconds between snapshots |
| `--http` | — | health/metrics listener |
| `--idle-secs` | `600` | final snapshot + exit with no participants; `0` keeps it open |

**Required by this RFC, not implemented yet** (the stack must add them before
the limits and guarantees below hold):

- `--snapshot-ops` (500) and a 64 MiB journal trigger — one snapshot policy
  with `--snapshot-secs` instead of the time-only default;
- `--allow-remote` — refuse a non-loopback `--listen` unless given, and require
  a TLS terminator behind it;
- `--origin` — `Origin` allowlist on the WebSocket upgrade;
- `--metrics-token` — bearer token for `/metrics` when it is not loopback;
- `--allow-dev-tokens` — an explicit switch for `--dev-token`, refused in
  release builds and when `--secret-file` is set;
- a `0600` requirement on `--secret-file`, a minimum secret length, and the
  verification-only/asymmetric key mode.

The journal and the rest of the state are **derived from `--state`**, not
separate flags: `<state>.jsonl` (journal), `<state>.version` (last committed
version), `<state>.meta.json` (manifest: version, generation, hashes, dedup),
`<state>.sat` (sidecar of unmodelled ACIS records), and `prev/` (previous
generation). The manifest, sidecar and `prev/` are part of the RFC contract;
the current binary writes only the DXF and the version file.

## Transport and TLS

The listener speaks plain WebSocket and binds to loopback by default. The
deployment terminates WSS (nginx, Traefik, the cabinet's ingress) and forwards
to it; exposing it on a private interface requires `--allow-remote` and a TLS
terminator in front. The WebSocket upgrade checks an `Origin` allowlist
(`--origin`), because a browser is the only client that can be tricked into
cross-origin upgrades. The token travels in the first message, not in the URL,
so a proxy log never holds it.

## Authentication and roles

The first message is `hello` with a JWT. The server verifies HS256 with the
cabinet's session secret (or a verification-only key) and pins the algorithm;
`alg:none` and foreign algorithms are refused. **All claims are required**:
`iss`, `aud`, `sub`, `fileId`, `app`, `role`, `sessionId`, `iat`, `exp`; they are
compared with `--issuer`, `--audience`, `--file-id`, `--session`, and
`app == "cadcraft"`. The role comes from the token:

| role | may |
|---|---|
| `viewer` | receive the drawing, presence, resync; no operations, no commit |
| `editor` | operations on the drawing, commit |
| `owner` | the same, plus closing the session |

The role and `exp` are checked on **every** operation and on `ping`, not only at
`hello`: the engine has no read-only mode, and a lease that outlives the token
would be a hole. A revoked or expired token gets `rejected{code:"auth"}` and the
connection closes; revocation does not merely drop presence. The maximum lease
TTL is 10 minutes; `ping` renews it only while the token is still valid. An
unauthenticated `hello` is dropped after 5 seconds and does not consume a
participant slot.

`--dev-token` tokens are for tests: they need `--allow-dev-tokens`, a unique
`sub` per token, an explicit expiry, and they are refused when `--secret-file`
is present and the build is a release one.

## What a room refuses

The command policy is **default-deny**: only the addressable command set of
ADR-0018 (`line`, `circle`, `move`, `polyline`/`pline`, `erase`, `rect`, `copy`,
`rotate`, …) and the `primitive` dictionary execute. An unknown `kind`,
`target`, or a non-addressable command is `rejected{code:"unsupported"}` before
the drawing changes; a command line in a shared session is parsed into an op
with explicit addresses or refused.

The deny-list below is the set of names that must never be added to the
allowlist — not the primary filter:

- Commands that leave the drawing: `open`, `save`, `saveas`, `new`, `quit`,
  `recover`, `audit`, `purge`, `plot`, `export`, `script`, `undo`, `redo`,
  `xref`, `document.bytes`, `lisp*`, `ui.*`, `app.*`, `file.*`, `plot*`,
  `appload`, `wblock`, `tableexport`, `dataextraction`, `eattext`, `datalink`,
  `datalinkupdate`, `drawingrecovery`, `exportpdf`.
- Operations whose parameters exceed 1 MiB, or whose change set exceeds 5000
  objects: `rejected{code:"too-large"}`, nothing changes.
- A `structural` operation built on a version that has moved:
  `rejected{code:"stale", retryable:true}` with `hint.version`; the room does
  not queue or rebase it. The client catches up and resubmits the same intent
  once with the new `baseVersion` (`rfc.md` §4.4).

## State, snapshots and restarts

The authoritative drawing lives in memory. Every applied operation is appended
to the journal (`<state>.jsonl`, one JSON line per operation). Each record
carries `format`, `version`, `length`, `sha256`, so an incomplete tail is
discarded on start, while a gap or a corrupt record in the middle refuses the
start: the room does not come up with a wrong drawing, and the operator keeps
the previous generation.

State files are created `0600` in a private directory. The snapshot is a
**generation** of three parts: the DXF payload, the `.sat` sidecar with the
records the writer cannot emit back (SAT-text `3DSOLID`/`REGION`/`BODY` without
ACIS data-store entries, `rfc.md` §4.6), and the manifest. The manifest is
written last, after `fsync` of both payloads, and carries
`{format, version, generation, sha256:{dxf, sat}, recordIds, handles, fileId,
savedAt, dedup}`. A generation is valid only when both hashes match and the
manifest names it; otherwise `prev/` is read, and if both are invalid the room
refuses to join (ADR-0024). The journal is truncated **only** after the new
generation is durable and read back.

One policy replaces the old mix of `--snapshot-secs 30` and «500 operations or
5 minutes»: a snapshot is written at 500 operations, or every 5 minutes, or when
the journal reaches 64 MiB — whichever comes first; two generations are kept
(`current`, `prev/`). A client that resumes from a version older than the
snapshot is sent the snapshot; its unacknowledged queue is kept and re-sent, and
the `opId` window answers repeats. The dedup window lives in the manifest, so
compaction does not erase it; an evicted repeat gets `stale` and a resync, never
a second execution.

On start the snapshot is preferred over `--file`, and the journal tail is
replayed on top of it: a restart loses at most the operations after the last
snapshot, and that loss is named to the clients rather than hidden.

DXF keeps what CADCraft does not model (unknown entities, other applications'
objects, xdata) through the writer's raw pass; SAT-text solids without
data-store records do not survive the DXF writer alone and travel in the
sidecar. DWG through the bridge does not keep everything; the snapshot is DXF
for that reason, and `commit` may write either format.

Backup is a copy of the generation directory plus `--file`; restore is «put the
previous generation in place and start» — the room then serves that version and
clients resync to it.

## Commit

`commit{opId, expectedVersion}` writes the file from the authoritative drawing
and answers `commit-result`. `opId` makes a retry idempotent: the same `opId`
returns the stored result instead of rebuilding the file.

The payload path depends on size, because every message is limited to 1 MiB:

- up to 512 KiB of base64 (≈384 KiB raw): `{bytes, sha256, size, version,
  notSaved}` in one message;
- above that: `{uploadUrl:"/commit/<opId>", sha256, size, version, notSaved}`
  and no bytes. The cabinet fetches the file over HTTP `Range` with the same
  JWT, verifies `sha256`, and only then creates the version; the server drops
  the object after confirmation or by TTL. `snapshot` is chunked over the
  WebSocket for the same reason (`rfc.md` §4.2/§4.6).

`notSaved` is `{format, dropped:[{what, count}], sidecar}`, counted **before**
writing (`loss_on_save`): the cabinet shows it to the user, records it in the
audit, and keeps the previous version.

The external-change check has one owner: the cabinet. The server sends
`expectedVersion` and `sourceSha256`; the cabinet compares them under its own
file lock and answers `409`; the server relays
`rejected{code:"external-change", retryable:false}` and the client offers
«save a copy». The room never overwrites a file it did not build on.

## Health, metrics, limits

- `GET /healthz`, `GET /readyz` → `200 ok`. With `--idle-secs` (default 600) the
  process writes a final snapshot and exits after that long with no participants;
  a participant who connects resets the timer.
- `GET /metrics` (Prometheus text) on loopback by default, or with
  `Authorization: Bearer $--metrics-token` when the endpoint is exposed:
  connections, applied/rejected operations, errors, version, journal length,
  participants.
- Limits are the single table of `rfc.md` §9: 8 participants, 50 operations/s
  per room and per `sub` with `rate-limited{hint.retryAfterMs}`, 1 MiB per
  message, 8 MiB per delta, 5000 changed / 50 000 scanned objects per
  transaction, 30 s presence TTL, 2 MiB per-participant send queue (disconnect
  on overflow), 64 MiB journal, 512 MiB snapshot, 5 s unauthenticated `hello`
  timeout, 32 connections per IP.
- Readiness is notified asynchronously; the old 50 ms read poll is a fallback
  only, and its added latency and CPU cost are measured before being kept.

## Logs

The server prints its session and address at start and nothing per operation.
Application logs are structured JSON: `sub`, `opId`, `version` and error codes,
with control characters stripped and fields length-bounded. They never include
drawing content, tokens or user paths.

## What is still missing for a first production deployment

- The desktop app joins a session with `--collab-url`, `--collab-doc` and
  `CADCRAFT_COLLAB_TOKEN` (the environment keeps the token out of the process
  list), sends every command that is not a UI/view command through
  `collab_command`, draws the other participants' cursors and names in the space
  being shown, and maps undo to the server's per-user step; the interactive
  command line is refused in a shared session, and the web build's flag is not
  wired yet.
- WSS termination tested against the cabinet's ingress, the `Origin` allowlist
  exercised, and the cabinet's ACL and secret provisioning end to end.
- The large-commit path (`uploadUrl`, `Range`, `sha256`) and a >512 KiB test
  fixture.
- The web build's session messages (`cadcraft:session`, presence, commit) through
  `craft-web-host` in `storytold/craft-libs`.
- 200k-entity measurements for the limit table (`study.md` §8).
