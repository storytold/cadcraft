# Hosting CADCraft for the web

`cadcraft-web-<version>.zip` (from the GitHub release, or `packaging/web/package.sh`) holds a
static site in `cadcraft-web-<version>/`:

| File | What it is |
|---|---|
| `index.html` | The page. It loads everything through relative URLs. |
| `cadcraft-web-<hash>.js` | wasm-bindgen glue (generated, ES module) |
| `cadcraft-web-<hash>_bg.wasm` | The app, about 13 MB, or 5 MB with compression |
| `_headers`, `.htaccess` | Sample header rules for Netlify/Cloudflare Pages and Apache |

There is no server-side code. Upload the folder's contents anywhere that serves static files.

## Any path works

All URLs in `index.html` are relative (`public_url = "./"` in `apps/cadcraft-web/Trunk.toml`),
so the site works at a domain root (`https://example.com/`), under a prefix
(`https://example.com/tools/cadcraft/`) and from a CDN bucket. The asset names carry a content
hash, so they can be cached forever. Only `index.html` needs revalidation.

## Required server settings

- **MIME type:** serve `.wasm` as `application/wasm`. Browsers refuse to stream-compile it under
  any other type, and the app then loads slowly or not at all. Serve `.js` as `text/javascript`.
  Most hosts already do both. For nginx, check that `mime.types` has `application/wasm wasm;`.
- **Compression:** turn on gzip or Brotli for `.wasm`, `.js` and `.html`. That takes the
  download from about 13 MB to about 5 MB. You can also precompress (`brotli -k *.wasm`) and let
  the server send `Content-Encoding: br`.
- **Caching:** `Cache-Control: public, max-age=31536000, immutable` on the hashed `.wasm` and
  `.js` files, and `no-cache` on `index.html`.
- **HTTPS:** WebGPU (and the clipboard) only work in a secure context, which means `https://`
  or `http://localhost`. Over plain HTTP elsewhere, the app falls back to WebGL2.
- **No special isolation headers:** CADCraft doesn't use `SharedArrayBuffer`, so it doesn't
  need `Cross-Origin-Opener-Policy` or `Cross-Origin-Embedder-Policy`. If your site already sends
  COEP `require-corp`, also send `Cross-Origin-Resource-Policy: same-origin` (or `cross-origin`
  when the files live on a CDN) on the app's files.

nginx example:

```nginx
location /cadcraft/ {
    types { application/wasm wasm; text/javascript js; text/html html; }
    gzip on;
    gzip_types application/wasm text/javascript text/html;
    location ~* \.(wasm|js)$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
    location ~* index\.html$ { add_header Cache-Control "no-cache"; }
}
```

Local test: `python3 -m http.server 8765` inside the folder, then open http://localhost:8765/.

## Embedding in a page (iframe)

```html
<iframe
  src="https://example.com/cadcraft/"
  title="CADCraft image editor"
  style="width: 100%; height: 720px; border: 0;"
  allow="fullscreen; clipboard-read; clipboard-write"
  allowfullscreen>
</iframe>
```

- The app fills the iframe and follows its size, so size the iframe and not the app.
- Keyboard shortcuts go to the iframe after the user clicks into it, as with any embedded app.
- **Cross-origin embeds** work. Preferences are kept in the iframe's `localStorage`. Browsers
  that partition or block third-party storage may forget them between visits, and the app
  then starts with defaults.
- **Sandboxed iframes** need at least
  `sandbox="allow-scripts allow-same-origin allow-downloads allow-popups"`. Without
  `allow-same-origin` there's no storage. Without `allow-downloads`, Save and Export (browser
  downloads) are blocked.
- Don't send `X-Frame-Options: DENY` or a `frame-ancestors` CSP that excludes the embedding page.

## Host mode: let the page open and store drawings

A page can take over the file side of the app instead of the browser's downloads. The iframe
embed above opts in by adding `?host=parent` to the iframe's URL
(`https://example.com/cadcraft/?host=parent`); the app then talks to its same-origin framing
page over `postMessage`, with plain window messages that carry a `type`:

| Message | Direction | Fields |
|---|---|---|
| `cadcraft:ready` | app → page | `version`: the app is listening; send a drawing now |
| `cadcraft:open` | page → app | `name`, `bytes` (an `ArrayBuffer` or `Uint8Array`): open like File › Open |
| `cadcraft:save` | app → page | `name`, `bytes` (a transferred `ArrayBuffer`): every file the app would otherwise download (SAVE, SAVEAS, exports). The page stores it and reports failures itself. |
| `cadcraft:dirty` | app → page | `dirty`: unsaved changes appeared or went away |
| `cadcraft:failed` | app → page | `error`: the app couldn't start |

In host mode the app starts on the Start tab instead of an empty Drawing1, so the page should
guard leaving it while changes are unsaved (that is what `cadcraft:dirty` reports).

### Same-document host mode (no iframe)

A page can also run the app in its own document — its own `<canvas id="cadcraft_canvas">`
inside a component, no iframe — by loading the wasm-bindgen module itself. It opts in with
`data-host="page"` on that canvas — the exact value `page`; anything else, and an unframed
`?host=parent`, keeps the normal app with downloads. The behaviour is the same as the parent
bridge, except the transport is DOM `CustomEvent`s on the canvas instead of `postMessage`: the
app dispatches `cadcraft:ready`, `cadcraft:save`, `cadcraft:dirty` and `cadcraft:failed` on the
canvas with the message fields (without the `type`) as the event's `detail`, and the page
dispatches a `cadcraft:open` event with `detail: { name, bytes }` to open a drawing. Events
don't bubble, so two editors on one page can't hear each other; listen on the canvas itself.

```html
<canvas id="cadcraft_canvas" data-host="page" style="width: 100%; height: 600px; border: 0;"></canvas>
```

```js
import init from "./cadcraft-web-<hash>.js"; // the wasm-bindgen glue shipped with the app

const canvas = document.getElementById("cadcraft_canvas");
// The app announces itself while `init` runs, so add the listeners first.
canvas.addEventListener("cadcraft:ready", () => {
  // The app is listening; hand it a drawing of the page's own now, if you have one.
  canvas.dispatchEvent(new CustomEvent("cadcraft:open", {
    detail: { name: "plan.dxf", bytes: drawingBytes }, // ArrayBuffer or Uint8Array
  }));
});
// Every file the app would otherwise download (SAVE, SAVEAS, exports) arrives here.
canvas.addEventListener("cadcraft:save", (event) => {
  const { name, bytes } = event.detail; // bytes is an ArrayBuffer
  storeFile(name, bytes);
});
canvas.addEventListener("cadcraft:dirty", (event) => {
  warnBeforeLeaving(event.detail.dirty);
});
canvas.addEventListener("cadcraft:failed", (event) => {
  showError(event.detail.error);
});

await init({ module_or_path: "./cadcraft-web-<hash>_bg.wasm" });
```

## Renderer selection and fallback flags

CADCraft renders with wgpu. It uses **WebGPU** when the browser has it and falls back to
**WebGL2** on its own. URL query flags override this, and they work on the iframe `src` too:

| Flag | Effect |
|---|---|
| *(none)* | WebGPU if available, otherwise WebGL2 |
| `?webgl` | Force the WebGL2 backend (useful when a WebGPU driver misbehaves) |
| `?cpu` | Force the CPU canvas path (slowest, most compatible) |

For example: `<iframe src="https://example.com/cadcraft/?webgl" ...>`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.
