//! Browser downloads: hand bytes to the user as a file, through web-sys only (a `Blob`, an object
//! URL and a temporary `<a download>` link). This is the web's file route for everything that
//! produces a file: drawing saves today (installed as the engine's
//! [`cadcraft_engine::cmd::file::set_deliver`] hook), plots next.

use wasm_bindgen::JsCast as _;
use web_sys::js_sys;

/// How long the object URL stays valid after the click; browsers read it asynchronously.
const REVOKE_AFTER_MS: i32 = 60_000;

/// Start a browser download of `bytes` as a file called `name` with MIME type `mime`.
///
/// `Ok` once the download was triggered. Where it lands (or whether the user cancels a "save as"
/// prompt of the browser) is up to the browser and can't be observed from the page.
pub fn download(name: &str, mime: &str, bytes: &[u8]) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a file name is required".into());
    }
    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no browser document")?;
    let body = document.body().ok_or("the page has no body")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    if let Err(e) = click_link(&document, &body, &url, name) {
        let _ = web_sys::Url::revoke_object_url(&url);
        return Err(e);
    }
    let later = url.clone();
    let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&later);
    });
    if window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), REVOKE_AFTER_MS).is_err() {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
    Ok(())
}

fn click_link(document: &web_sys::Document, body: &web_sys::HtmlElement, url: &str, name: &str) -> Result<(), String> {
    let link = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not create a download link".to_string())?;
    link.set_href(url);
    link.set_download(name);
    link.set_attribute("style", "display:none").map_err(js_error)?;
    body.append_child(&link).map_err(js_error)?;
    link.click();
    link.remove();
    Ok(())
}

fn js_error(e: wasm_bindgen::JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}
