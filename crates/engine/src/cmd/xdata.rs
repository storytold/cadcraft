//! Other applications' extended data on entities: set, get and erase it by
//! application name (the `1001` head of each xdata chunk). The data
//! round-trips through the drawing file and follows copies, and the
//! reader keeps everything other applications left in the file — this is
//! the write path for that channel (the proof-of-concept surface of the
//! plug-in API's per-object data, with XRecords/extension dictionaries to
//! follow).

use cadcraft_doc::RawTag;
use serde_json::{Value, json};

use super::{CommandSpec, bad, str_param, targets};
use crate::{Result, Session};

const MAX_TAGS: usize = 4096;
const MAX_VALUE_BYTES: usize = 255;
/// The reader drops other applications' chunks beyond this many groups per
/// object (`io::dxf_raw::MAX_XDATA_GROUPS`), and treats several apps as
/// owned by the app itself — a command write into one of those names would
/// be silently lost on save and reload, so both must mirror the reader.
const MAX_XDATA_TOTAL: usize = 16_384;
const RESERVED_APPS: &[&str] = &["CADCRAFT", "ACAD", "ACDBBLOCKREPBTAG", "ACADANNOTATIVE", "ACCMTRANSPARENCY", "ACAECLAYERSTANDARD"];

/// Cut to the 255 bytes one `1000` string may hold, at a character boundary.
fn xdata_str(s: &str) -> String {
    let mut end = s.len().min(MAX_VALUE_BYTES);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// The xdata chunk of `app`: the `1001` head plus the groups before the next `1001`.
fn remove_app(app: &str, xdata: &mut Vec<RawTag>) -> bool {
    let mut next: Vec<RawTag> = Vec::with_capacity(xdata.len());
    let mut changed = false;
    let mut i = 0usize;
    while i < xdata.len() {
        let is_target = xdata[i].code == 1001 && xdata[i].value.trim().eq_ignore_ascii_case(app);
        let start = i;
        i += 1;
        while i < xdata.len() && xdata[i].code != 1001 {
            i += 1;
        }
        if is_target {
            changed = true;
        } else {
            next.extend(xdata[start..i].iter().cloned());
        }
    }
    *xdata = next;
    changed
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let h = *targets(s, p)?.first().ok_or_else(|| bad("xdata.set", "`handle` is required (or a selection)"))?;
    let app = app_param(p, "xdata.set")?;
    let tags = tags_param(p)?;
    let groups = tags.len() + 1;
    // The reader keeps at most MAX_XDATA_TOTAL groups per object and drops
    // everything beyond, so size the merge here instead of losing data on
    // save: the kept chunks plus the new chunk must fit the cap.
    let kept = {
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("xdata.set", format!("no object {}", h.hex())))?;
        let mut count = 0usize;
        let mut i = 0usize;
        while i < e.common.xdata.len() {
            let is_target = e.common.xdata[i].code == 1001 && e.common.xdata[i].value.trim().eq_ignore_ascii_case(&app);
            let start = i;
            i += 1;
            while i < e.common.xdata.len() && e.common.xdata[i].code != 1001 {
                i += 1;
            }
            if !is_target {
                count += i - start;
            }
        }
        count
    };
    if kept + groups > MAX_XDATA_TOTAL {
        return Err(bad("xdata.set", format!("the object keeps {kept} other groups; this chunk would push it past the {MAX_XDATA_TOTAL} cap")));
    }
    let d = s.doc_mut()?;
    d.modify_entity(h, |e| {
        remove_app(&app, &mut e.common.xdata);
        // Remove again is already done; append the new chunk after every
        // other application's chunk.
        let mut merged: Vec<RawTag> = Vec::with_capacity(e.common.xdata.len() + groups);
        let mut head: Vec<RawTag> = vec![RawTag { code: 1001, value: app }];
        head.extend(tags);
        merged.extend(head);
        merged.extend(e.common.xdata.iter().cloned());
        e.common.xdata = merged;
    })
    .map_err(|e| bad("xdata.set", e.to_string()))?;
    Ok(json!({ "set": true, "handle": h.hex(), "groups": groups }))
}

fn run_get(s: &mut Session, p: &Value) -> Result<Value> {
    let h = *targets(s, p)?.first().ok_or_else(|| bad("xdata.get", "`handle` is required (or a selection)"))?;
    let app = p.get("app").and_then(Value::as_str).map(|s| s.trim().to_string());
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| bad("xdata.get", format!("no object {}", h.hex())))?;
    let mut chunks: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while i < e.common.xdata.len() && e.common.xdata[i].code == 1001 {
        let name = e.common.xdata[i].value.trim().to_string();
        let start = i + 1;
        let mut j = start;
        while j < e.common.xdata.len() && e.common.xdata[j].code != 1001 {
            j += 1;
        }
        let matches = app.as_ref().is_none_or(|want| name.eq_ignore_ascii_case(want));
        if matches && j > start {
            let taglist: Vec<Value> = e.common.xdata[start..j].iter().map(|t| json!([t.code, t.value])).collect();
            chunks.push(json!({ "app": name, "tags": taglist }));
        }
        i = j;
    }
    Ok(json!({ "xdata": chunks }))
}

fn run_erase(s: &mut Session, p: &Value) -> Result<Value> {
    let h = *targets(s, p)?.first().ok_or_else(|| bad("xdata.erase", "`handle` is required (or a selection)"))?;
    let app = app_param(p, "xdata.erase")?;
    let d = s.doc_mut()?;
    d.modify_entity(h, |e| {
        remove_app(&app, &mut e.common.xdata);
    })
    .map_err(|e| bad("xdata.erase", e.to_string()))?;
    Ok(json!({ "erased": true, "handle": h.hex() }))
}

fn app_param(p: &Value, command: &str) -> Result<String> {
    let app = str_param(p, "app")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad(command, format!("`{command}` requires `app` as a non-empty string")))?
        .trim()
        .to_string();
    // The reader models these apps' data itself: the reader keeps the
    // chunk out of `Common::xdata`, so writing into one would be lost on
    // save and reload without the caller knowing.
    if RESERVED_APPS.iter().any(|r| r.eq_ignore_ascii_case(&app)) {
        return Err(bad(command, format!("`{app}` is reserved: its extended data is owned by the app itself")));
    }
    Ok(app)
}

fn tags_param(p: &Value) -> Result<Vec<RawTag>> {
    let list = p.get("tags").and_then(Value::as_array).ok_or_else(|| bad("xdata.set", "`tags` is required: [[code, \"value\"], …]"))?;
    if list.len() > MAX_TAGS {
        return Err(bad("xdata.set", format!("at most {MAX_TAGS} tag groups")));
    }
    let mut out: Vec<RawTag> = Vec::with_capacity(list.len().min(1024));
    for (ix, t) in list.iter().enumerate() {
        let bad_pair = |what: &str| bad("xdata.set", format!("tag {ix}: {what}"));
        let arr = t.as_array().ok_or_else(|| bad_pair("expected [code, \"value\"]"))?;
        let code = arr.first().and_then(Value::as_i64).ok_or_else(|| bad_pair("integer code expected"))?;
        let value = arr.get(1).and_then(Value::as_str).ok_or_else(|| bad_pair("value string expected"))?;
        let code = i32::try_from(code).map_err(|_| bad_pair("code out of range"))?;
        if !((1000..=1071).contains(&code) && code != 1001) {
            return Err(bad_pair("xdata codes run 1000..=1071, and the 1001 head belongs to the app name"));
        }
        if code == 1002 && value != "{" && value != "}" {
            return Err(bad_pair("1002 groups hold \"{\" or \"}\""));
        }
        out.push(RawTag { code, value: xdata_str(value) });
    }
    Ok(out)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xdata.set", "Set Object Extended Data", run_set)
            .params("{handle | handles, app, tags: [[code, \"value\"], …]} → {set, handle, groups}: the app's chunk replaces its previous one; codes 1000..=1071 (1002 holds \"{\" or \"}\"), no 1001; at most 4096 tags, strings cut at 255 bytes")
            .noundo(),
        CommandSpec::new("xdata.get", "Object Extended Data", run_get)
            .params("{handle | handles, app?} → {xdata: [{app, tags}]} (all applications when `app` is left out)")
            .noundo(),
        CommandSpec::new("xdata.erase", "Erase Object Extended Data", run_erase)
            .params("{handle | handles, app}: erases the app's chunk")
            .noundo(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app_chunk<'a>(xdata: &'a [RawTag], app: &str) -> Option<&'a [RawTag]> {
        let mut i = 0usize;
        let mut out = None;
        while i < xdata.len() && xdata[i].code == 1001 {
            let name = xdata[i].value.as_str();
            let start = i + 1;
            let mut j = start;
            while j < xdata.len() && xdata[j].code != 1001 {
                j += 1;
            }
            if name.eq_ignore_ascii_case(app) && out.is_none() {
                out = Some(&xdata[start..j]);
            }
            i = j;
        }
        out
    }

    #[test]
    fn set_replaces_only_its_own_chunk() {
        let mut xdata = vec![
            RawTag { code: 1001, value: "OTHER".to_string() },
            RawTag { code: 1002, value: "{".to_string() },
            RawTag { code: 1030, value: "1.0".to_string() },
            RawTag { code: 1002, value: "}".to_string() },
        ];
        remove_app("OTHER", &mut xdata);
        assert!(xdata.is_empty() && !remove_app("OTHER", &mut xdata));
        // A merge into a chunk that shares xdata with another app keeps both, own first.
        let mut mine = vec![RawTag { code: 1001, value: "MYAPP".to_string() }];
        remove_app("MYAPP", &mut mine);
        mine.insert(0, RawTag { code: 1001, value: "MYAPP".to_string() });
        mine.push(RawTag { code: 1000, value: "product-42".to_string() });
        let tag = app_chunk(&mine, "MYAPP");
        assert_eq!(tag, Some(&[RawTag { code: 1000, value: "product-42".to_string() }][..]));
    }

    #[test]
    fn tags_param_validates() {
        let tags_json = |v: Value| tags_param(&json!({ "tags": v }));
        assert!(tags_json(json!([[1000, "s"]])).is_ok());
        assert!(tags_json(json!([[1001, "s"]])).is_err());
        assert!(tags_json(json!([[999, "s"]])).is_err());
        assert!(tags_json(json!([[1072, "s"]])).is_err());
        assert!(tags_json(json!([[1002, "oops"]])).is_err());
        assert!(tags_json(json!([[1000]])).is_err());
        assert!(tags_json(json!([[1004, "a"]])).is_ok());
    }

    #[test]
    fn app_param_rejects_reserved_and_empty() {
        let ok = |app: &str| app_param(&json!({ "app": app }), "xdata.set");
        assert!(ok("MAGI").is_ok());
        assert!(ok("").is_err());
        assert!(ok("  ").is_err());
        assert!(ok("CADCRAFT").is_err());
        assert!(ok("ACAD").is_err());
    }
    #[test]
    fn tags_validate() {
        let tags = |v: Value| tags_param(&json!({ "tags": v })).unwrap();
        let tags_err = |v: Value| tags_param(&json!({ "tags": v })).is_err();
        assert_eq!(tags(json!([[1000, "hello"]])).len(), 1);
        assert!(tags_err(json!([[1001, "x"]]))); // app head, not a tag
        assert!(tags_err(json!([[999, "x"]]))); // below the xdata range
        assert!(tags_err(json!([[1072, "x"]]))); // beyond the range
        assert!(tags_err(json!([[1002, "oops"]]))); // only braces
        assert!(tags_err(json!([[1000]]))); // missing value
        // Strings longer than the byte budget are cut, at a character
        // boundary, to what one 1000 group may hold.
        let long = "ä".repeat(200);
        let tag = tags(json!([[1000, long]])).remove(0);
        assert_eq!(tag.value.chars().count(), 127);
        assert!(tag.value.len() <= 255);
    }

    #[test]
    fn remove_app_keeps_other_chunks() {
        // Build by hand: OTHER keeps its data, MINE is replaced.
        let mut x = vec![RawTag { code: 1001, value: "OTHER".to_string() }, RawTag { code: 1000, value: "a".to_string() }];
        assert!(!remove_app("MINE", &mut x));
        x.push(RawTag { code: 1001, value: "MINE".to_string() });
        x.push(RawTag { code: 1000, value: "b".to_string() });
        assert!(remove_app("MINE", &mut x));
        assert_eq!(x.len(), 2, "OTHER's two groups kept, MINE's gone");
        assert_eq!(x[0].value, "OTHER");
        assert!(!remove_app("MINE", &mut x));
    }
}
