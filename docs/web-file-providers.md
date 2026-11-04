# Optional external file providers (web)

The web editor can delegate native project persistence to one **trusted same-origin host**.
The capability is **disabled by default**. Merely loading the editor exposes a protocol object;
it does not enable storage, render provider Recents, change ordinary local saves, or contact a server.
A host must explicitly call registerProvider. No server, account system, login, VM, domain,
filesystem mount or cloud vendor is built into this feature.

## Contract

window.cadcraftFileProviders has protocolVersion 1 and app cadcraft. Events are dispatched on window
as cadcraft-file-provider CustomEvents; detail contains bounded metadata and, on save,
a Uint8Array named bytes. The host subscribes before registering, and can discover an already-ready
editor through the protocol object. The capability is independent of the optional control API.

The application command `ui.provider.open` accepts `{"resource":"resource-id"}` and returns
the queued provider request ID. Home and File-menu provider Recents both dispatch through
`CadApp::run`; rejected reads remain in the status and command history instead of a one-frame
inline label. This command uses the current account's resource list and never opens a local path.

| Method | Arguments / result |
| --- | --- |
| registerProvider | (providerId, label, namespace) -> connectionId. Repeating the exact identity is idempotent. |
| unregisterProvider | No arguments. Refused while an operation is unresolved; clears account resources and bindings. |
| setEnabled | (boolean). Suspends new provider choices/Recents without discarding pending work. |
| setResources | (connectionId, resources). Replace this account's native project list. |
| requestOpen | (resourceId) -> requestId. IDs must occur in the current connection's resource list. |
| openResource | (requestId, filename, Uint8Array). Replies only to a live, single-use open request. |
| cancelOpen | (requestId, error). Empty error means cancellation. |
| resolveSaveDialog | (requestId, destination, filename). destination is provider, device or cancel. |
| saveResult | (requestId, status, resourceId). status is saved, download, failed or cancel. |
| snapshot | Returns provider/namespace/connectionId, enabled, resources, documents and pending state. |

The namespace must identify the authenticated account, not its display name. A new connection ID
and per-page cryptographic request nonce prevent old account/page callbacks from matching new work.
This is correlation and separation of identities, **not authentication**. The host and its backend
must authenticate users, authorize every list/read/write, and scope durable storage by account.
Do not trust a filename, user-supplied account ID, browser page or request ID as an authorization check.
Only the hosting page's own script environment can use this capability; no postMessage listener,
foreign-origin message channel or filesystem authority is added.

Every script executing in the editor page, including third-party analytics, can call registerProvider
and receive saved drawings through provider events. Load only scripts you trust with drawing contents.
The host/backend remains responsible for authenticating users and authorizing each account-scoped operation.

A resource is an object with id, name and optional updatedAt strings. IDs are opaque and never used as
paths. The list is limited to 100 unique resources. Names must be native .dxf filenames, at most
255 UTF-8 bytes, with no path separators or control characters. IDs/namespaces are at most 160 ASCII
characters (letters, numbers, dash, underscore, dot, colon). Labels are at most 100 UTF-8 bytes.
Native captures/read responses are limited to 80 MiB; retained transfer bytes and pending work are bounded.
The WASM adapter checks string/array lengths before copying host input and accepts only Uint8Array bytes.

## Save and Save As

Every interactive Save or Save As while enabled captures a native project and emits saveDialog.
The event carries requestId, documentId, revision, intent and suggested filename. The host presents
its own choice of **provider / device / cancel**, then calls resolveSaveDialog. The editor owns the
captured state while the choice is open, so changing tabs or editing cannot silently change the target.
No save happens merely because a dialog was opened or a byte capture was produced.

Choosing provider emits save with the immutable native bytes, filename, intent, documentId, revision,
providerId, namespace, connectionId and requestId. Save includes a previously bound resourceId;
Save As requests a distinct resource. The provider must finish its durable write before returning
saveResult(requestId, saved, resourceId). An ACK cannot nominate a document or revision, cannot be
replayed, and cannot bind a resource already owned by another open document. Failed/cancelled writes
keep edits unsaved. Later edits remain dirty when an older capture succeeds. There is no pretend local
path for an external resource.

Choosing device emits save with destination device and the capture. The host hands the bytes to the device using the
browser's ordinary download mechanism, then returns download as the result. This is a terminal transfer
result, **not durable provider confirmation**, and does not clear the document's saved checkpoint.
Save Copy, Export and programmatic commands keep their existing local behavior. With the capability
disabled or unregistered, the original browser/native save flow remains in use.

Provider Save/Save As captures native DXF. Without explicit `linkedFilesV1` negotiation, capture still refuses external images and xrefs because DXF paths do not contain their bytes. The optional extension below carries the native DXF and a complete authorized dependency set. It grants no filesystem or network authority and does not replace DXF with an archive or proprietary main format.
Use the existing local Copy/Export commands for interchange formats. The host must preserve the native
extension; changing a suffix cannot turn native bytes into PDF, SVG or another interchange format.
Native capture uses the existing IoHooks / write_as DXF encoder. The legacy ?host=parent bridge is preserved; the new capability is installed only outside that mode so two hosts do not compete for Save. DWG and interchange outputs remain in the original local flow.

Normal close/revert actions wait or refuse while provider work is unresolved. The browser can still
force-close/reload a tab; the host should provide an unsaved-work warning and must not acknowledge a
write it cannot recover. This bridge does not add automatic saving, cross-client conflict resolution,
offline persistence, autosave recovery or an inactivity/login implementation.

## Open and Recents

The existing Home screen and File > Open Recent display a separate provider section while enabled.
Local Recents retain their own storage and behavior. Publish only the current account's native files
with setResources; the editor never scans a VM or guesses a path from a name.

A click emits openRequested with requestId and resourceId. After authorization, the provider fetches
bytes and responds with openResource using that request ID. The editor parses the native project before
emitting opened or openFailed. Selecting an already-bound open resource focuses its existing document
without replacing unsaved edits. Unknown, duplicate or cancelled request replies are rejected.
The host handles failed fetches using cancelOpen rather than leaving the operation pending forever.

Completion events are saved, downloaded, saveFailed, saveCancelled, opened, openFailed and openCancelled.
Events associated with an account include providerId, namespace and connectionId. Storage keys should
use the provider/account/resource identities, never display names, and the backend should use versioning
or conditional writes if it allows concurrent clients.

## Optional linkedFilesV1

A provider that can store, retrieve and export a **whole native drawing set** explicitly calls
`configureLinkedFiles(connectionId, 1)` while the current connection is idle. Advertising the
capability does not negotiate it. Disconnect resets negotiation. Ordinary unregistered, disabled,
unnegotiated and explicit local DWG routes retain their existing behavior. This extension does
not implement a browser DWG codec, an image renderer, font installation or a storage backend.

| Method | Arguments / result |
| --- | --- |
| configureLinkedFiles | (connectionId, 1). Negotiate only on the current idle connection. |
| completeResourceSet | (connectionId, requestId, set). Supply an authorized complete set for a pending read, frozen save or explicit refresh. |
| cancelResourceSet | (connectionId, requestId, error). Empty error cancels. A nonempty error fails the owned operation. |
| saveResourceSetResult | (connectionId, requestId, status, resourceId, setDigest, versionToken). `saved` acknowledges the exact whole set with its opaque committed version. `download` confirms only device transfer. `failed`/`cancel` grant no persistence credit. |

The host must authorize **every supplied file and relation** for the authenticated account and
requested resource before calling `completeResourceSet`. Filenames, native reference strings,
logical keys, hashes and connection IDs are not authorization credentials. No native path is read
and no URL is fetched to fill a missing file. Returning a partial set cannot open or mark a drawing
saved. The current connection ID is required on every new extension reply, including failures.

The set has this bounded JSON structure (each `bytes` value is an actual `Uint8Array`, not JSON):

```text
{
  main: { logicalKey, name, sha256, bytes? },
  files: [{ logicalKey, kind, sha256, bytes }],
  edges: [{ fromKey, rawReference, role, targetKey }],
  versionToken
}
```

`main.name` is a native display filename. `main.logicalKey` is its stable location inside the
logical project tree. The main's native DXF bytes are mandatory on open. On save or refresh,
they may be omitted: the editor's captured main bytes and SHA256 are authoritative. Any supplied
main must match that capture exactly. The reply must retain its name and any established logical root. For a new drawing without an
authorized resource context, `resourceSetNeeded.mainLogicalKey` is empty. The host chooses the
first authorized logical root (for example `plans/Main.dxf`) so native relative parent paths can
remain valid. That root is then fixed for the captured set and future saves. On open,
`versionToken` identifies the committed whole-set version and uses the protocol's opaque ASCII-ID
syntax. For a save or refresh input it is metadata, not a durable acknowledgement.

Each attachment kind is `dxf`, `raster` or `font`. Each relation role is `xref`, `image`, `font` or
`bigFont`. A single relation covers repeated occurrences of the same raw reference and role within
one native file. The editor decodes **every reachable DXF**, verifies all its actual dependencies,
checks each role/kind, rejects missing, duplicate, ambiguous or unreachable declarations and only
then changes the document. Nested, unloaded and overlay xrefs require their bytes too. Cycles are
retained as relations and visited once. Xrefs inside the provided context are not expanded through
the native filesystem. `XOPEN` uses the supplied DXF and its reachable authorized set. Its display
name comes from the logical key's last component when that is a native `.dxf` filename (ASCII
case-insensitive extension, at most 255 UTF-8 bytes, no separators or controls), otherwise
`Reference.dxf`. The resulting document title and unbound provider Save suggestion retain that
name, including extension case. These display names never grant filesystem authority. Native
xref reload/path/attach/bind operations in that context are refused rather than reading a path or
binding unloaded geometry. Non-provider drawings keep their existing native xref behavior.

Logical keys are case-distinct, collision-checked project identifiers, never OS paths. Each uses
forward slashes, at most 1024 UTF-8 bytes and 32 components, with no empty, `.` or `..` components,
leading slash, colon, backslash or controls. Case-folded collisions are refused. Relative native
references resolve within this tree, including bounded `..` to a parent inside it. Absolute or
platform-specific native reference strings need an explicit authorized relation to a logical key.
They never become file authority. The host must safely encode logical keys when storing/exporting
on a filesystem, rather than concatenating untrusted paths or assuming OS filename portability.

The native closure supports model, layout and owned-block image paths, DXF xrefs and explicit
font-file/bigfont paths. Font families and built-in names, including `CADCraft Stroke`, are
metadata and do not trigger automatic font collection. Text-style paths, unnamed shape-font STYLE records and MTEXT font-switch
paths require authorized font bytes. Raw STYLE file references are checked as well as the decoded
model, including portable-font filenames the codec normalizes back into a family name. The native reader retains inert source-file provenance alongside that decoded family. Linked Save/refresh restores the original raw STYLE filename and family metadata while the family remains unchanged, including its directory spelling. A deliberate font change supersedes that provenance and requires the newly referenced authorized bytes. Ordinary local writers, style JSON and semantic equality do not change. Authorized font bytes stay opaque and are never installed.
Permitted raster leaves have PNG, JPEG, GIF, BMP, TIFF or WebP signatures. SVG/container dependencies,
external CSV data links and unresolved external plot-style files are unsupported. Built-in or
embedded plot-style tables and ordinary printer/device names are metadata. No bitmap or font
appearance parity is asserted by preserving original bytes.

Completeness is deliberately limited to recognized native data. The real codec consumes its
modeled IMAGEDEF and other standard records. The closure permits the retained resource-free
DICTIONARY/ACDBDICTIONARYWDFLT/ACDBPLACEHOLDER record shapes and known header strings. Unknown
entities/classes/objects, material pointers, extension xdata, proxy/data-store content, skipped records, incomplete
reads and unclassified string-valued headers are explicitly unsupported. They are not silently
certified as dependency-free DXF. This policy does not claim support for arbitrary foreign DXF.

A complete set has at most 256 files including main, 1024 relations and depth 16 beyond main.
Depth admission checks simple paths separately from the decode-once native cache. An edge closing
its current active cycle is retained without expanding that cycle indefinitely. A proof memo may
be reused only for the same node and the same reachable active ancestors, after its continuations
were proved safe, and only at an equal or shallower depth. A visit blocked by another ancestor
cannot hide a longer shortcut path. A reachable-unused-node upper bound avoids enumerating legal
cycles that cannot exceed the remaining depth.

Each graph first receives cheap, general upper-bound certificates for the additional edges of a
simple path starting at each node:

- The number of reachable vertices minus one.
- The largest component after removing that start vertex from the undirected graph induced by
  its reachable vertices. A simple path cannot return through its start, so its remaining
  vertices must stay within one such component. Ignoring edge direction can only enlarge it.
- A verified vertex cover of the undirected non-self-loop graph. Restrict that cover to the
  reachable vertices and let its size be k. A path starting outside the cover has at most 2k
  edges; one starting inside has at most 2k-1, because two vertices outside a cover cannot be
  adjacent. The maximum-uncovered-degree greedy cover need not be minimal; every retained
  edge is checked to touch the resulting cover before its bound is used.

Certificates ignore other active ancestors, a conservative superset of the permitted path.
Their minimum, together with the reachable-unused-node bound, can prove a continuation safe
without enumerating it. Otherwise the same context-sensitive completed-proof DFS runs. These
are general graph properties, not recognizers for named graph families. They positively admit
bidirected K(8,64) (72 files, 1,024 relations, maximum simple depth16), clique17, and K(8,60)
plus the small-side directed clique (68 files, 1,016 relations, maximum simple depth16).
The native K(8,64) regression source requires successful Open, cached Save, original authorized
attachment/edge preservation and a complete-set ACK; a computation refusal is not success.

Preprocessing reuses the directed reachable masks: at most 256 directed reachability scans,
256 start-removal component scans, and 256 greedy degree passes. The undirected adjacency has
at most 2,048 entries (at most twice the 1,024 declared relations); duplicate neighbors and
self-loops are removed only for these path certificates. Actual relation preservation is
unchanged. These bounds are precomputed once, not recomputed at every DFS visit. The masks
use four u64 words per node and the certificate vector has at most 256 entries.

These sufficient certificates do not decide every valid graph. Remaining cyclic simple-path
analysis can still require combinatorial work. Each validation processes at most 1,048,576
depth visits (including certified, cached and cycle-closing visits) and retains at most 65,536
compact proof-memo entries.
Memo saturation only disables further memo insertion. Work exhaustion returns the explicit error
`linked depth analysis exceeded 1048576 visits; nesting was not certified`, before Open/refresh
or a complete-set save can succeed. This is an analysis refusal, not evidence of excessive depth
or a partial positive verdict. Some otherwise valid closures can reach this work limit. The
resource policy for those cases remains a maintainer decision; no universal cyclic-graph support
or latency is claimed. Active recursion has at most 18 frames, including an attempted depth17
or cycle-closing arrival. These structural work and memo limits do not count as native file
contents in the separate 80 MiB retained-content budget.
Each native DXF is also limited to 4,000,000 tags in the raw reference scan.
The aggregate retained **file-content** budget is 80 MiB, including main captures, attachments,
older resource generations, undo snapshots and pending owners. Immutable content is SHA256-interned
and shared by captures/checkpoints instead of copying every snapshot. A process-wide content pool
also caps distinct resident files at 1024. Retaining old generations can therefore refuse a new
capture/refresh before any existing bytes are discarded. These are content and structural limits,
not a promise that total application/decoder/JavaScript memory fits in 80 MiB. The WASM adapter
checks all declared paths, counts and aggregate lengths before copying payload buffers, then
verifies actual hashes. It also admits additional resident content against the existing budget.

### Captures, refresh and whole-set results

A negotiated Save/Save As still starts with the ordinary owned `saveDialog`. Choosing provider or
device first rebuilds a complete closure from authorized cached resources and the immutable native
capture. If bytes or mappings are missing, `resourceSetNeeded` carries `operation: save`, frozen
native `bytes`, `mainLogicalKey`, `mainSha256`, filename, destination, documentId and revision.
The host supplies the complete authorized closure for **that capture**, never a later live drawing.
A stale/missing-resource failure or cancellation leaves the drawing and saved checkpoint intact.

The command `ui.provider.refreshResources` (File > Refresh linked files) explicitly requests new
attachment bytes for the current native drawing. Its `resourceSetNeeded` has `operation: refresh`.
It passes through the actual app command and consumer, applies only to the captured document and
revision, and records an undoable resource generation without changing selection or geometry.
A changed revision, closed owner, active command/LISP draft or open Block Editor refuses the reply. Refresh cannot
implicitly import another account's retained resources. A deliberate Save As can export them to a
new account. Account identity and the connection that explicitly granted the supplied bytes remain attached to resource generations, including across disconnect. Reconnecting even the same account does not renew that old grant. A provider destination emits resourceSetNeeded with requiresCurrentGrant: true and must receive a newly authorized completeResourceSet reply on the current connection before reusing those bytes. Explicit refresh can likewise obtain a current grant. Cached open bytes remain available for an honest complete-set device export; a device transfer neither renews the provider grant nor advances its checkpoint. Connection IDs correlate the host assertion and are not authorization credentials.

The final `save` event contains the native main `bytes`, `mainLogicalKey`, `setDigest`, a metadata
`manifest`, and `attachments` with **all** authorized original bytes, logical keys, kinds and hashes.
It also contains the frozen document/revision, account/connection, destination, filename, intent,
resourceId and expectedVersion. A bound Save carries the last committed whole-set version for the
host's conditional write. Save As requires a distinct resource, and cannot steal an open alias.

`setDigest` is lowercase SHA256 of compact UTF-8 JSON with recursively lexicographically sorted
object keys, no whitespace, and the escaping/number encoding used by serde_json. The manifest has
`schema: cadcraft-linked-files-v1`, `name`, `main`, `files`, and `edges`. File rows contain logicalKey,
kind, byte length (`bytes`) and lowercase sha256. Files sort by logical key. Edges sort by the tuple
(fromKey, rawReference, role, targetKey). Authority and version tokens are excluded from this content
digest. The same bytes/relations/name therefore yield the same digest across reconnects and ACKs.

Only `saveResourceSetResult(..., saved, ..., exactDigest, committedVersion)` can acknowledge a negotiated
full-set save. The ordinary single-file `saveResult` cannot do so. The host/backend must persist
main **and every attachment and relation atomically as one version** before returning `saved`.
The editor checks correlation and the whole-set receipt. It cannot prove host disk durability,
authorization or a remote conditional write. No such backend or packaging service is implemented here.
An accepted ACK checkpoints only its captured native drawing and resources. Later drawing/resource
edits remain dirty, even if another tab is active. Failed, cancelled, stale, duplicate or wrong-digest
results cannot manufacture a saved checkpoint. Refresh itself does not acknowledge persistence.

For destination `device`, the same event exposes every attachment for a complete project export.
The host must export the native DXF plus its authorized attachment tree and relations, preserving
logical paths. A display filename does not rewrite native reference paths or rename the logical
root. A DXF-only download is **not** a complete linked project. Common-host packaging and filesystem
durability are future host responsibilities. `download` never advances the provider checkpoint.
Explicit ordinary local exports still mean their existing formats, not an implicit attachment bundle. For a document carrying provided linked context, an ordinary explicit-path or disabled-provider drawing save is a drawing-only copy: it reports a command-line omission warning and returns linkedCopy={completeProject:false, filesOmitted:[logical keys], relationsOmitted:count, checkpointAdvanced:false}. It preserves the linked saved drawing/resources, path/title and existing dirty state. It does not mark an incomplete copy as a saved project; use the negotiated complete-set device destination for that export. Documents without provided resource context retain their original local/default-off/native-DWG save behavior.

Native and resource generations participate in undo/redo, dirty checks and captured checkpoints.
Resource replacement preserves selection, and old captures retain their own immutable
bytes. Newer generations are never overwritten by an old successful receipt. Unknown/late replies
are rejected; bounded queue pressure retains retryable owners. Account changes/disconnect wait for
queued/in-flight terminal delivery. Recoverable JS input or output asset-copy exceptions use the
actual app failure consumer and correlated terminal, including when the normal command inbox is
full. Accepted synchronous callbacks remain authoritative if dispatch subsequently throws. Fatal
OOM/WASM traps and listener exceptions swallowed by the DOM remain outside that recovery guarantee.

## Recoverable publication failures

Both the byte publisher and linked-input copier use explicit catch-enabled `Uint8Array.from` imports. Recoverable JavaScript
copy/allocation errors become returned errors, are logged, and do not bypass later events in the
batch. This is not recovery from fatal memory exhaustion, WASM traps or process termination; DOM
listener exceptions swallowed by the browser are not assumed to propagate from dispatchEvent.

A failed `save` or `saveDialog` moves only its still-owned capture into a bounded failure inbox.
It retains every captured document alias in finishing-saves ownership until the actual CAD
application consumes the failed Action and queues its correlated `saveFailed` terminal. Retained
bytes are discarded. No saved checkpoint, binding or durable acknowledgement is manufactured.
Public action/result admission checks are unchanged. Previously admitted saves/refreshes (at most 16) and reads (at most 16) can
await failure consumption separately, including when the ordinary queue is full. Repaint is requested through
the installed editor context. Accepted synchronous results/choices remain authoritative.

A failed `openRequested` retires only an unanswered read and retains its correlated `openFailed`
terminal across event-queue capacity limits. An already queued open response survives a later
dispatch error. Fresh choices/reads can retry after failure consumption; IDs are never reused.

Busy/account ownership includes ordinary and linked-resource completion event types while queued or in a taken
publication batch. Batch protection is acquired atomically and released independently for nested
publishers. Hosts must not unregister synchronously from a completion listener, including the last
terminal listener in the batch: publication ownership is released only after the whole batch returns.
Defer `unregisterProvider()` to a later task, for example with `setTimeout(..., 0)`, and wait for all
other outstanding operations and terminal deliveries to finish. Independent save/open admission rules
are unchanged. The publisher still performs two bounded drains in order.

## Validation

Regression fixtures cover the default-off gate, single-use/late replies, account reconnection, resource
scope and alias rejection, cancellation, immutable captures, switching tabs, edits after capture and
conservative device results. The source includes the WASM adapter and native encoder/decoder integration.
Run the required repository CI and the actual Window fixtures on the complete candidate. This document does not claim those checks
have been executed merely because fixtures are present.

New source regressions cover full ordinary action queues, a bounded failure inbox, captured aliases,
failed chooser/read publication, accepted reentrant callbacks, current/nested terminal ownership
and recovery through a full event queue. The native application fixture uses the real DXF codec and
CAD failure consumer without altering process-global format hooks, preserving another active drawing
and later edits. The WASM fixture uses the exported window API, production publisher, actual CadApp
logic and native DXF encoder/decoder. Rust closures produce real JS RangeErrors for byte and metadata
dispatch failures; catch-enabled calls restore exact property descriptors before assertions. It
observes continuation, correlated failure terminals, a nonempty DXF retry and opening that payload
under a distinct resource, without marking later edits saved.

The actual Window fixtures are in
[`browser_tests.rs`](../apps/cadcraft-web/src/file_providers/browser_tests.rs) and its
[`linked_browser_tests.rs`](../apps/cadcraft-web/src/file_providers/linked_browser_tests.rs) submodule.
With a real browser and the `wasm-bindgen-test` browser runner configured, run both through the existing
WASM main harness:

```sh
cargo test --locked --offline -j1 -p cadcraft-web --bin cadcraft-web --target wasm32-unknown-unknown file_providers::browser_tests:: -- --nocapture
```

These are authored fixtures, not execution receipts. The complete candidate's real browser run remains required;
native tests or WASM compilation alone do not establish the window assertions. The fixture does not
run a rendered WebRunner, prove backend durability or simulate true out-of-memory failure. The web
test dependency is wasm32-only; the UI's existing workspace IO crate is a test-only dependency for
real drawing-codec fixtures. Their actual lock graph must be generated and reviewed before checks.

Simulated user Save/Save As fixtures use the existing Null parameter form; JSON object parameters continue to mean explicit programmatic operations and never acquire dialogs. Added regressions cover normalized source-font paths and identical bytes through actual Save/native decode/reopen, first provider capture, deliberate font edits, inert ordinary codec/JSON behavior, explicit/disabled incomplete copies, shared-DAG descendant depth and same-account reconnection grants. Added cycle-context regressions reject the 32-file shortcut/tail depth17 case in declaration and actual native Open, and accept its 31-file depth16 boundary through actual Open/cached Save/full-set ACK in both declaration orders. Engine regression sources cover dense legal-cycle pruning and explicit analysis-budget refusal. General certificate sources additionally require successful K(8,64), nonbipartite small-cover and many-petal-cycle admission without exponential enumeration, while a larger-cover graph must still reject excessive nesting.

The `provider_linked_resources` native integration binary exercises actual CadApp commands,
DXF encode/decode, original PNG decode, complete cyclic resource closure, resource undo/redo,
later edits, conditional full-set receipts, device transfer, native-path isolation, count/depth
refusals, bounded failure consumption and reentrant accepted replies. Synthetic fixture bytes are
original and embedded in Rust source. The additional real-window marker is
`file_providers::browser_tests::linked::real_window_linked_files_preserve_native_closure_and_recover_attachment_delivery`.
It calls the exported API and production publisher, injects catchable JS RangeErrors during input
copy and after main output copy, consumes actual CadApp failures, retries and reopens the complete
native/resource bytes under another resource. These new sources are unexecuted until validated.
Neither the new marker nor the existing Window fixture runs a rendered WebRunner or certifies
host persistence, real memory exhaustion, bitmap rendering or browser DWG support.

Contributor credit: @alejocapo04.

## Native UI evidence

The existing recent-files test harness covers the real Start page with the optional provider absent
and connected, retaining the separate device history in both states. It uses synthetic drawing names
and metadata, with no storage or authentication backend. The native label assertions also run without
a GPU when the output variable is unset.

```sh
CRAFT_PROVIDER_UI_FIXTURES=/absolute/output cargo test --locked -p cadcraft-ui-egui --test recent_files provider_recents_render_separately_from_device_history -- --exact --test-threads=1
```

This saves `cadcraft-provider-recents-off.png` and `cadcraft-provider-recents-on.png` using
egui_kittest's existing offscreen renderer. Keep the generated review artifacts outside Git.
