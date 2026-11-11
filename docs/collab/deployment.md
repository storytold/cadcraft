# CADCraft collaboration: deployment

How `cadcraft-collab-server` runs, what it expects from the cabinet and what it
guarantees. The protocol and its decisions are in `rfc.md` and
`.craft/bus/decisions/0001-0010`.

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
  --session 6f1c… \
  --secret-file /run/secrets/office-sync-hs256 \
  --issuer cabinet --audience office-sync \
  --state /var/lib/office-sync/6f1c.dxf \
  --assets /var/lib/office-sync/assets \
  --snapshot-secs 30 \
  --http 127.0.0.1:8788
```

`--dev-token token:editor` (repeatable) replaces `--secret-file` for tests and a
single-tenant deployment. Never use it where the cabinet signs tokens.

## Commit callback (M87)

With a cabinet beside the room, the room publishes every commit itself; no tab
has to stay open:

```bash
  --cabinet-url http://cabinet-backend:8080/api/v1/internal/office/sessions/{sessionId}/commit \
  --cabinet-token-file /run/secrets/craft-callback-token \
  --file-id 42
```

`{sessionId}` and `{fileId}` in the URL are substituted; the body is
`{fileId, name, bytes(base64), sha256, version, expectedVersion?}` and the
cabinet answers `{version, etag}`. The next commit sends that `etag` as
`expectedVersion`.

- `200` → the room broadcasts `committed` with the cabinet's `version`/`etag`
  and **does not send the file to the clients**; a hosted session never writes a
  second local copy.
- `409` → `commit-conflict` to the requester; the room keeps the built file for
  `save-copy`, and never overwrites the cabinet.
- `401`/`413` → an error with `cabinet-unauthorized`/`cabinet-too-large`, no
  retry. A network failure or `5xx` is retried with the same body
  (`--cabinet-attempts`, default 3).
- The token comes from a file, never from the process list or a log. `https://`
  is refused: TLS belongs to the proxy.

Without `--cabinet-url` the room keeps the old behaviour: `commit-result` with
the file goes to the requester, and the desktop app writes it locally.

## Transport and TLS

The listener speaks plain WebSocket; the deployment terminates WSS (nginx,
Traefik, the cabinet's ingress) and forwards to it on loopback or a private
network. The token travels in the first message, not in the URL, so a proxy log
never holds it.

## Authentication and roles

The first message is `hello` with a JWT. The server verifies HS256 with the
cabinet's session secret and, when given, the `iss` and `aud` claims. The role
comes from the token:

| role | may |
|---|---|
| `viewer` | receive the drawing, presence, resync; no operations, no commit |
| `editor` | operations on the drawing, commit |
| `owner` | the same, plus closing the session |

The role is checked on **every** operation, because the engine has no read-only
mode. Revoking access means closing the connection; the room drops the
participant's presence at once.

## What a room refuses

- Commands that leave the drawing: `open`, `save`, `saveas`, `new`, `quit`,
  `recover`, `audit`, `purge`, `plot`, `export`, `script`, `undo`, `redo`,
  `xref`, `document.bytes`, `lisp*`, `ui.*`, `app.*`, `file.*`, `plot*`.
- Operations whose parameters exceed 1 MiB, or whose change set exceeds 5000
  objects: `rejected{code:"too-large"}`, nothing changes.
- A `structural` operation built on a version that has moved:
  `rejected{code:"stale"}`, no retry storm; the room's order decides.

## State, snapshots and restarts

The authoritative drawing lives in memory. Every applied operation is appended
to a journal (`--state.jsonl`, one JSON line per operation); every
`--snapshot-secs` the room writes its DXF to `--state` (temporary file + rename)
with the version beside it (`--state.version`) and empties the journal, because
the snapshot now holds everything the journal did.

On start the snapshot is preferred over `--file`, and the journal tail is
replayed on top of it: a restart loses at most the operations after the last
snapshot. A gap in the journal (a line missing or out of order) is refused at
start rather than guessed at — the room does not come up with a wrong drawing;
the operator keeps the previous snapshot or reopens the file.

A client that resumes from a version older than the snapshot is sent a snapshot;
its queued operations are re-sent and the `opId` window answers repeats.

DXF keeps what CADCraft does not model (unknown entities, other applications'
objects, xdata) through the writer's raw pass; DWG through the bridge does not.
The snapshot is DXF for that reason; `commit` may write either format.

## Attachments (`--assets`)

`imageattach` never reads a file by a path a client names (ADR 0021). The room
takes the raster bytes in the operation (`data`, at most the 1 MiB message
limit) or by their content address (`asset`, SHA-256) from a store given with
`--assets <dir>`. The store keeps one directory per room (`<dir>/<session>/`),
names every blob `<sha256>.<ext>` with the extension taken from the bytes'
signature, and deduplicates by address. `assetPut` stores bytes (editor and
owner only) after checking the address against them; `assetGet` serves them to
any authenticated participant of that room, viewers included. There is no
message that takes a file name.

Limits are per room: 256 MiB per blob, 1000 blobs, 2 GiB in total. They are
counted from the disk, so a restart neither loses blobs nor resets the quota;
a blob that was already stored is accepted without taking more space. A missing
id answers `not-found`; an address that is not 64 lower-case hex characters, a
blob that does not hash to its id, and bytes that are not a PNG/JPEG/BMP/TIFF
answer `invalid`; a limit answers `limit`. A symlink inside the store that
resolves outside the room's directory is treated as missing. Without
`--assets`, `imageattach` still accepts inline `data` but refuses an `asset`
address: the engine must never fall back to opening the label as a file.

## Commit

`commit{expectedVersion}` writes the file from the authoritative drawing and
answers `commit-result` with the bytes (base64), the version and `notSaved`: what
the format could not keep, counted **before** writing. The cabinet must:

1. keep the previous version and write the new one;
2. show `notSaved` to the user and record it in the audit;
3. refuse a commit built on a moved version (the server answers `stale`), and a
   file changed outside the session (`409` from the cabinet).

## Health, metrics, limits

- `GET /healthz`, `GET /readyz` → `200 ok`. With `--idle-secs` (default 600) the
process writes a final snapshot and exits after that long with no participants; a
participant who connects resets the timer.
- `GET /metrics` (Prometheus text): connections, applied/rejected operations,
  errors, version, journal length, participants.
- Limits: 8 participants per room, 1 MiB per message, 5000 objects per
  operation, 30 s presence TTL, 50 ms read poll. The read poll bounds latency of
  a broadcast to one socket; the per-participant send queue keeps a slow writer
  from blocking the room.

## Logs

The server prints its session and address at start and nothing per operation.
Application logs use `sub`, `opId`, version and error codes; they never include
drawing content, tokens or user paths.

## The browser build (`apps/cadcraft-web`)

The web build is the same `CadApp` behind eframe's web runner; its host bridge is
`craft-web-host` (storytold/craft-libs). A page hosts it with `?host=parent` (an
iframe) or `data-host="page"` on the canvas and speaks the messages of
`INTEGRATION.md` §6 with the `cadcraft:` prefix.

- **The document** arrives as `cadcraft:open { name, bytes }` with the session
  (`INTEGRATION.md` §5.1). It opens in the window — it is not a room operation,
  and the room refuses `open` — and the room's snapshot replaces it when the join
  completes, so a second participant sees the edits made before it arrived. Once
  the room's drawing has joined, a late `open` is refused instead of forking the
  shared drawing; the cabinet reloads the editor to switch files.
- **Commands** run locally only when the UI handles them (view, settings,
  `document.switch`); everything else goes through `collab_command` as an
  operation of the room. A viewer's operation is refused before it is sent and by
  the room. An operation id carries a prefix unique to the window: the room
  answers a repeated `op_id` with the first answer (ADR 0003) and keys that window
  by the id alone, so two windows — or one after a reload — must not both send
  `local-0`, or the second edit is answered with the first one's change set and
  never applied.
- **Presence** carries the cursor, the selection, the space and the layer at the
  server's 10 Hz pace, and the participant list reaches the cabinet as
  `session-state` and `presence`. A shared session keeps painting at 10 Hz while
  it is settled and at 16 ms while it is not: the snapshot joins in bounded
  slices per frame and the reconnect timers run from socket callbacks.
- **Save** asks the room to publish the file (`commit{expectedVersion}`); the room
  writes it through the cabinet's callback (M87) and answers with the cabinet's
  file version, which the app shows and records. A room without that callback
  answers with the bytes, and the page's cabinet stores them (`cadcraft:commit`,
  `cadcraft:committed`).
- **Reconnect** is the transport's: the socket redials with backoff and the last
  version, the room answers with the journal tail or a snapshot, and the cabinet
  renews the ticket by sending a fresh `session`.
- **The flag.** The build opens no session of its own: it joins one only when the
  hosting page sends `cadcraft:session`. With no host, or a host that sends none
  (the cabinet's `CRAFT_COLLAB_ENABLED` is off), the build stays single-user:
  Open reads the browser's file picker, Save downloads, nothing dials a socket.

## What is still missing for a first production deployment

- The desktop app joins a session with `--collab-url`, `--collab-doc` and
  `CADCRAFT_COLLAB_TOKEN` (the environment keeps the token out of the process
  list), sends every command that is not a UI/view command through
  `collab_command`, draws the other participants' cursors and names in the space
  being shown, and maps undo to the server's per-user step; the interactive
  command line is refused in a shared session. Unlike the web build, the desktop
  window keeps the file it opened when the room's snapshot arrives, so two
  desktops opened from different files can diverge.
- WSS termination tested against the cabinet's ingress, and the cabinet's ACL
  and secret provisioning exercised end to end.
- Access is revoked at the next reconnect: the room checks the token when a
  socket opens and has no revoke message, so closing a participant's access
  takes effect when its socket is redialled.
