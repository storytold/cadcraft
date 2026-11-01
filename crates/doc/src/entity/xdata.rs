//! Extended entity data (xdata): the groups 1000–1071 other applications attach to an entity,
//! kept raw in [`Common::xdata`](super::Common::xdata) as whole applications, each from its
//! `1001` group on. This module reads them per application, replaces or removes one
//! application's data, and checks data that comes from outside (a command's parameters, a LISP
//! list) before it goes onto an entity: input is hostile, so codes, values, brace nesting and
//! sizes are all checked and nothing here panics.

use super::RawTag;

/// Most xdata groups one object may carry (AutoCAD allows 16 KB of xdata per object).
pub const MAX_XDATA_GROUPS: usize = 16_384;
/// Longest application name, in bytes.
pub const MAX_XDATA_APP_NAME: usize = 255;
/// Longest string value (groups 1000 and 1003), in bytes: what one DXF group holds.
pub const MAX_XDATA_STRING: usize = 255;
/// Longest binary chunk (group 1004), in bytes.
pub const MAX_XDATA_CHUNK: usize = 127;

/// Applications whose xdata CADCraft writes itself, from its own model, when a drawing is saved.
/// It is never kept as an entity's raw xdata (the reader models it) and can't be set as such:
/// a copy written beside the generated data would contradict it.
pub const RESERVED_XDATA_APPS: &[&str] = &["CADCRAFT", "ACADANNOTATIVE", "ACCMTRANSPARENCY", "ACAECLAYERSTANDARD"];

/// Whether `name` (any case) is one of [`RESERVED_XDATA_APPS`].
pub fn is_reserved_xdata_app(name: &str) -> bool {
    let up = name.trim().to_ascii_uppercase();
    RESERVED_XDATA_APPS.contains(&up.as_str())
}

/// Whether application `name`'s xdata on an entity of `kind` is written from the model rather
/// than kept raw: the [`RESERVED_XDATA_APPS`] on any entity, and `ACAD` on a dimension, whose
/// `ACAD` run (`DSTYLE`) holds its dimension-style overrides. The reader models those too, so
/// raw groups set under such a name would be dropped on save; they are refused instead.
pub fn is_model_xdata_app(name: &str, kind: &super::EntityKind) -> bool {
    is_reserved_xdata_app(name) || (name.trim().eq_ignore_ascii_case("ACAD") && matches!(kind, super::EntityKind::Dimension(_)))
}

/// Why an application name or a run of xdata groups was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdataError(pub String);

impl std::fmt::Display for XdataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for XdataError {}

fn err<T>(msg: impl Into<String>) -> Result<T, XdataError> {
    Err(XdataError(msg.into()))
}

/// Check an application name: not empty, at most [`MAX_XDATA_APP_NAME`] bytes, no control
/// characters and none of the characters a symbol table name can't hold.
pub fn check_xdata_app_name(name: &str) -> Result<(), XdataError> {
    let name = name.trim();
    if name.is_empty() {
        return err("an xdata application name must not be empty");
    }
    if name.len() > MAX_XDATA_APP_NAME {
        return err(format!("an xdata application name may have at most {MAX_XDATA_APP_NAME} bytes"));
    }
    if name.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c)) {
        return err(format!("`{name}` is not a valid xdata application name (no control characters or <>/\\\":;?*|,=`)"));
    }
    Ok(())
}

/// Whether `code` is an xdata group code an application's data may hold (`1001`, the
/// application name, is not one of them: it starts an application).
pub fn is_xdata_group(code: i32) -> bool {
    matches!(code, 1000 | 1002..=1005 | 1010..=1013 | 1020..=1023 | 1030..=1033 | 1040..=1042 | 1070 | 1071)
}

/// Check the groups of one application (after its `1001`): every code an xdata code with a
/// value of its kind, control strings `{` and `}` balanced, points complete (a `101x` group
/// followed by its `102x` and `103x`), at most [`MAX_XDATA_GROUPS`] groups.
pub fn check_xdata_groups(groups: &[RawTag]) -> Result<(), XdataError> {
    if groups.len() > MAX_XDATA_GROUPS {
        return err(format!("at most {MAX_XDATA_GROUPS} xdata groups per application"));
    }
    let mut depth: u32 = 0;
    let mut i = 0;
    while let Some(t) = groups.get(i) {
        let v = t.value.trim();
        match t.code {
            1000 | 1003 => {
                if t.value.len() > MAX_XDATA_STRING {
                    return err(format!("group {}: a string may have at most {MAX_XDATA_STRING} bytes", t.code));
                }
                if t.value.chars().any(|c| c == '\n' || c == '\r') {
                    return err(format!("group {}: a string may not hold a line break", t.code));
                }
            }
            1002 => match v {
                "{" => depth = depth.saturating_add(1),
                "}" => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return err("group 1002: a `}` without its `{`"),
                },
                _ => return err("group 1002 must be `{` or `}`"),
            },
            1004 => {
                if v.len() > MAX_XDATA_CHUNK * 2 || v.len() % 2 != 0 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return err(format!("group 1004 must be hex text of an even length, at most {MAX_XDATA_CHUNK} bytes"));
                }
            }
            1005 => {
                if v.is_empty() || v.len() > 16 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return err("group 1005 must be a hex handle");
                }
            }
            1010..=1013 => {
                let (y, z) = (groups.get(i + 1), groups.get(i + 2));
                let complete = y.is_some_and(|y| y.code == t.code + 10) && z.is_some_and(|z| z.code == t.code + 20);
                if !complete {
                    return err(format!("group {}: a point needs its {} and {} groups", t.code, t.code + 10, t.code + 20));
                }
                for p in [Some(t), y, z].into_iter().flatten() {
                    real(p)?;
                }
                i += 3;
                continue;
            }
            1020..=1023 | 1030..=1033 => return err(format!("group {} may only follow its {} group", t.code, t.code - (t.code - 1010) / 10 * 10)),
            1040..=1042 => {
                real(t)?;
            }
            1070 => {
                let ok = v.parse::<i64>().is_ok_and(|n| i16::try_from(n).is_ok());
                if !ok {
                    return err("group 1070 must be a 16-bit integer");
                }
            }
            1071 => {
                let ok = v.parse::<i64>().is_ok_and(|n| i32::try_from(n).is_ok());
                if !ok {
                    return err("group 1071 must be a 32-bit integer");
                }
            }
            1001 => return err("group 1001 (an application name) can't be inside an application's data"),
            c => return err(format!("group {c} is not an xdata group code (1000–1071)")),
        }
        i += 1;
    }
    if depth != 0 {
        return err("group 1002: a `{` without its `}`");
    }
    Ok(())
}

fn real(t: &RawTag) -> Result<f64, XdataError> {
    match t.value.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => err(format!("group {} must be a finite number", t.code)),
    }
}

/// The applications of an xdata run, in order: each name as written and its groups after the
/// `1001`. Groups before the first `1001` belong to nobody and are left out.
pub fn xdata_apps(xdata: &[RawTag]) -> Vec<(&str, &[RawTag])> {
    let mut out = Vec::new();
    let mut start: Option<(usize, &str)> = None;
    for (i, t) in xdata.iter().enumerate() {
        if t.code != 1001 {
            continue;
        }
        if let Some((s, name)) = start.take() {
            out.push((name, xdata.get(s + 1..i).unwrap_or(&[])));
        }
        start = Some((i, t.value.trim()));
    }
    if let Some((s, name)) = start {
        out.push((name, xdata.get(s + 1..).unwrap_or(&[])));
    }
    out
}

/// The groups of application `app` (any case): those of its first run.
pub fn xdata_of<'a>(xdata: &'a [RawTag], app: &str) -> Option<&'a [RawTag]> {
    xdata_apps(xdata).into_iter().find(|(name, _)| name.eq_ignore_ascii_case(app.trim())).map(|(_, g)| g)
}

/// Replace the xdata of application `app` (any case) with `groups`, or remove it (`None`, or no
/// groups). Every run of that application goes; new data is appended after the other
/// applications' data, with its `1001` group. The groups are taken as given: check them with
/// [`check_xdata_groups`] first when they come from outside.
pub fn set_xdata_app(xdata: &mut Vec<RawTag>, app: &str, groups: Option<&[RawTag]>) {
    let app = app.trim();
    let mut kept: Vec<RawTag> = Vec::with_capacity(xdata.len());
    let mut skipping = false;
    for t in xdata.drain(..) {
        if t.code == 1001 {
            skipping = t.value.trim().eq_ignore_ascii_case(app);
        }
        if !skipping {
            kept.push(t);
        }
    }
    if let Some(groups) = groups.filter(|g| !g.is_empty()) {
        kept.push(RawTag { code: 1001, value: app.to_string() });
        kept.extend(groups.iter().cloned());
    }
    *xdata = kept;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn t(code: i32, value: &str) -> RawTag {
        RawTag { code, value: value.into() }
    }

    #[test]
    fn apps_are_split_at_their_names() {
        let x = vec![t(1001, "A"), t(1000, "one"), t(1070, "1"), t(1001, "b"), t(1000, "two")];
        let apps = xdata_apps(&x);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].0, "A");
        assert_eq!(apps[0].1.len(), 2);
        assert_eq!(apps[1], ("b", &x[4..]));
        assert_eq!(xdata_of(&x, "B"), Some(&x[4..]));
        assert_eq!(xdata_of(&x, "c"), None);
        assert!(xdata_apps(&[t(1000, "stray")]).is_empty());
    }

    #[test]
    fn set_replaces_removes_and_appends() {
        let mut x = vec![t(1001, "A"), t(1000, "one"), t(1001, "B"), t(1000, "two"), t(1001, "a"), t(1000, "dup")];
        set_xdata_app(&mut x, "a", Some(&[t(1070, "7")]));
        assert_eq!(x, vec![t(1001, "B"), t(1000, "two"), t(1001, "a"), t(1070, "7")]);
        set_xdata_app(&mut x, "B", None);
        assert_eq!(x, vec![t(1001, "a"), t(1070, "7")]);
        set_xdata_app(&mut x, "C", Some(&[]));
        assert_eq!(x.len(), 2);
        set_xdata_app(&mut x, "A", None);
        assert!(x.is_empty());
    }

    #[test]
    fn checks_accept_every_kind_and_refuse_bad_data() {
        let good = vec![
            t(1000, "text"),
            t(1002, "{"),
            t(1003, "Layer"),
            t(1004, "DEADBEEF"),
            t(1005, "1A2"),
            t(1010, "1.5"),
            t(1020, "2"),
            t(1030, "0"),
            t(1040, "2.5"),
            t(1041, "1e3"),
            t(1042, "-1"),
            t(1070, "32767"),
            t(1071, "2147483647"),
            t(1002, "}"),
        ];
        assert_eq!(check_xdata_groups(&good), Ok(()));
        for bad in [
            vec![t(1002, "}")],
            vec![t(1002, "{")],
            vec![t(1002, "x")],
            vec![t(1001, "APP")],
            vec![t(999, "comment")],
            vec![t(8, "layer")],
            vec![t(1010, "1"), t(1020, "2")],
            vec![t(1020, "2")],
            vec![t(1010, "nan"), t(1020, "2"), t(1030, "3")],
            vec![t(1040, "inf")],
            vec![t(1040, "x")],
            vec![t(1070, "40000")],
            vec![t(1071, "3000000000")],
            vec![t(1004, "ABC")],
            vec![t(1004, "XY")],
            vec![t(1005, "")],
            vec![t(1005, "zz")],
            vec![t(1000, &"x".repeat(256))],
            vec![t(1000, "a\nb")],
            vec![t(1000, "x"); MAX_XDATA_GROUPS + 1],
        ] {
            assert!(check_xdata_groups(&bad).is_err(), "{:?}", bad.first());
        }
        assert_eq!(check_xdata_app_name("PE_URL"), Ok(()));
        assert_eq!(check_xdata_app_name("My App-1.0$"), Ok(()));
        for bad in ["", " ", "a*b", "a\tb", "x/y", &"n".repeat(256)] {
            assert!(check_xdata_app_name(bad).is_err(), "{bad:?}");
        }
        assert!(is_reserved_xdata_app("cadcraft") && !is_reserved_xdata_app("ACAD"));
    }
}
