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

Provider Save/Save As captures one native DXF. DXF stores external image/drawing references as paths, not their bytes. To prevent an incomplete upload being marked saved, this bridge refuses provider capture when the modeled drawing contains external images or xrefs; bind/remove those references first. Provider Open checks the decoded drawing for those same modeled references before running the native Open command, so provider bytes cannot trigger its automatic image/xref filesystem loaders. A refusal leaves existing documents unchanged and reports the correlated openFailed terminal. Ordinary local Open keeps its existing native behavior.

Single-file transfers inherit the existing [DXF codec limits](file-format-parity.md). Font files, including SHX/bigfonts, and other dependencies are not collected or packaged. The bridge does not certify arbitrary foreign DXF as lossless or scan unmodeled records for dependencies. It adds no linked-file negotiation, resource-set checkpoints, attachment bundle or atomic multi-file save. The host receives the one DXF produced by the native writer and remains responsible for its durable storage.
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

## Recoverable publication failures

The byte publisher calls an explicit catch-enabled `Uint8Array.from` import. Recoverable JavaScript
copy/allocation errors become returned errors, are logged, and do not bypass later events in the
batch. This is not recovery from fatal memory exhaustion, WASM traps or process termination; DOM
listener exceptions swallowed by the browser are not assumed to propagate from dispatchEvent.

A failed `save` or `saveDialog` moves only its still-owned capture into a bounded failure inbox.
It retains every captured document alias in finishing-saves ownership until the actual CAD
application consumes the failed Action and queues its correlated `saveFailed` terminal. Retained
bytes are discarded. No saved checkpoint, binding or durable acknowledgement is manufactured.
Public action/result admission checks are unchanged. At most 16 previously admitted captures can
await failure consumption separately, including when the ordinary queue is full. Repaint is requested through
the installed editor context. Accepted synchronous results/choices remain authoritative.

A failed `openRequested` retires only an unanswered read and retains its correlated `openFailed`
terminal across event-queue capacity limits. An already queued open response survives a later
dispatch error. Fresh choices/reads can retry after failure consumption; IDs are never reused.

Busy/account ownership includes all seven completion event types while queued or in a taken
publication batch. Batch protection is acquired atomically and released independently for nested
publishers. Hosts must defer unregisterProvider until the publication batch returns and all outstanding
operations and terminal deliveries finish. Independent save/open admission rules are unchanged.
The publisher still performs two bounded drains in order.

## Validation

Regression fixtures cover the default-off gate, single-use/late replies, account reconnection, resource
scope and alias rejection, cancellation, immutable captures, switching tabs, edits after capture and
conservative device results. The source includes the WASM adapter and native encoder/decoder integration.
Run the required repository CI and the actual Window fixture on the complete candidate. This document does not claim those checks
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

The native provider_format_boundaries integration binary covers the real DXF Save/Save As refusal,
self-contained capture and ACK, ordinary explicit DWG and disabled-provider local save behavior. Its
provider_open_refuses_external_images_and_xrefs_before_native_path_loading fixture exercises provider
Open refusals for model/layout/block images and xrefs without adding a document or binding.

The actual Window fixture is in
[browser_tests.rs](../apps/cadcraft-web/src/file_providers/browser_tests.rs). With a real browser and
the wasm-bindgen-test browser runner configured, run the existing WASM main harness:

```sh
cargo test --locked --offline -j1 -p cadcraft-web --bin cadcraft-web --target wasm32-unknown-unknown file_providers::browser_tests:: -- --nocapture
```

Simulated user Save/Save As uses the existing Null parameter form. JSON object parameters continue
to mean explicit programmatic operations and never acquire dialogs.

These are authored fixtures, not execution receipts. Real browser/runner execution remains required;
native tests or WASM compilation alone do not establish the window assertions. The fixture does not
run a rendered WebRunner, prove backend durability or simulate true out-of-memory failure. The web
test dependency is wasm32-only; the UI's existing workspace IO crate is a test-only dependency for
real drawing-codec fixtures. The existing locked graph is retained; its candidate must be reviewed
before checks.

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
