//! Xdata as JSON, for `entities`, `properties` and `properties.set`: an object by application,
//! `{"APP": [[code, value], …]}`, with each group's value typed by its code: strings for 1000,
//! 1002 (`{` / `}`), 1003 (a layer name), 1004 (hex) and 1005 (a handle), a point `[x, y, z]`
//! for 1010–1013 (its 1020 and 1030 groups folded in), numbers for 1040–1042, integers for
//! 1070 and 1071. Setting takes the same form; `null` for an application removes its data.

use cadcraft_doc::{MAX_XDATA_GROUPS, RawTag, check_xdata_app_name, check_xdata_groups, is_reserved_xdata_app, set_xdata_app, xdata_apps};
use serde_json::{Map, Value, json};

/// Most applications one `properties.set` call may set.
const MAX_APPS_PER_CALL: usize = 256;

/// Which applications' xdata a query returns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum XdataSelect {
    /// None (the default).
    #[default]
    None,
    /// Every application's.
    All,
    /// These applications' (any case); an entity without any of them doesn't match the query.
    Apps(Vec<String>),
}

impl XdataSelect {
    /// The `xdata` parameter of a query: `true`, `false`, an application name or a list of names.
    pub(crate) fn parse(v: Option<&Value>) -> Result<Self, String> {
        const MSG: &str = "`xdata` must be true, false, an application name or a list of names";
        match v {
            None | Some(Value::Null) => Ok(Self::None),
            Some(Value::Bool(b)) => Ok(if *b { Self::All } else { Self::None }),
            Some(Value::String(s)) => Ok(Self::Apps(vec![s.clone()])),
            Some(Value::Array(a)) if !a.is_empty() && a.len() <= MAX_APPS_PER_CALL => {
                a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| MSG.to_string())).collect::<Result<Vec<_>, _>>().map(Self::Apps)
            }
            Some(_) => Err(MSG.into()),
        }
    }

    /// Whether the selection narrows a query to entities carrying the named applications' data.
    pub(crate) fn filters(&self) -> bool {
        matches!(self, Self::Apps(_))
    }

    /// Whether an entity with this xdata passes the filter.
    pub(crate) fn matches(&self, xdata: &[RawTag]) -> bool {
        match self {
            Self::None | Self::All => true,
            Self::Apps(apps) => xdata_apps(xdata).iter().any(|(name, _)| apps.iter().any(|a| a.trim().eq_ignore_ascii_case(name))),
        }
    }

    /// The selected applications' data of an entity as JSON; `None` when there is none to show.
    pub(crate) fn json(&self, xdata: &[RawTag]) -> Option<Value> {
        let apps = match self {
            Self::None => return None,
            Self::All => None,
            Self::Apps(apps) => Some(apps),
        };
        let mut out = Map::new();
        for (name, groups) in xdata_apps(xdata) {
            if apps.is_some_and(|apps| !apps.iter().any(|a| a.trim().eq_ignore_ascii_case(name))) {
                continue;
            }
            // A second run of the same application (not something AutoCAD writes; `ACME` and
            // `acme` are one application, as `set_xdata_app` treats them) joins the first.
            let key = out.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
            match key.and_then(|k| out.get_mut(&k)) {
                Some(Value::Array(v)) => v.extend(groups_json(groups)),
                _ => {
                    out.insert(name.to_string(), Value::Array(groups_json(groups)));
                }
            }
        }
        (!out.is_empty()).then_some(Value::Object(out))
    }
}

/// Every application's data of an entity as JSON; `None` when it has none.
pub(crate) fn xdata_json(xdata: &[RawTag]) -> Option<Value> {
    XdataSelect::All.json(xdata)
}

fn number(t: &RawTag) -> Value {
    t.value.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map_or_else(|| json!(t.value), |v| json!(v))
}

fn integer(t: &RawTag) -> Value {
    t.value.trim().parse::<i64>().map_or_else(|_| json!(t.value), |v| json!(v))
}

/// One application's groups as `[code, value]` pairs, points folded.
fn groups_json(groups: &[RawTag]) -> Vec<Value> {
    let mut out = Vec::with_capacity(groups.len());
    let mut i = 0;
    while let Some(t) = groups.get(i) {
        let v = match t.code {
            1010..=1013 => {
                let y = groups.get(i + 1).filter(|y| y.code == t.code + 10);
                let z = groups.get(i + 2).filter(|z| z.code == t.code + 20);
                if let (Some(y), Some(z)) = (y, z) {
                    i += 3;
                    out.push(json!([t.code, [number(t), number(y), number(z)]]));
                    continue;
                }
                number(t)
            }
            1020..=1033 | 1040..=1042 => number(t),
            1070 | 1071 => integer(t),
            _ => json!(t.value),
        };
        out.push(json!([t.code, v]));
        i += 1;
    }
    out
}

/// What `properties.set`'s `xdata` parameter asks for: per application, its new groups (with
/// their `1001` left out) or `None` to remove it. Everything is checked here: names, codes, value
/// types and ranges, braces, sizes; reserved applications are refused.
pub(crate) fn xdata_from_json(v: &Value) -> Result<Vec<(String, Option<Vec<RawTag>>)>, String> {
    let Some(obj) = v.as_object() else { return Err("`xdata` must be an object {\"APP\": [[code, value], …] | null}".into()) };
    if obj.len() > MAX_APPS_PER_CALL {
        return Err(format!("`xdata` may name at most {MAX_APPS_PER_CALL} applications"));
    }
    let mut out = Vec::with_capacity(obj.len());
    for (app, groups) in obj {
        check_xdata_app_name(app).map_err(|e| e.to_string())?;
        if is_reserved_xdata_app(app) {
            return Err(format!("xdata application `{app}` is reserved: CADCraft writes its data itself"));
        }
        let groups = match groups {
            Value::Null => None,
            Value::Array(a) => Some(groups_from_json(app, a)?),
            _ => return Err(format!("xdata of `{app}` must be a list of [code, value] pairs, or null to remove it")),
        };
        out.push((app.trim().to_string(), groups));
    }
    Ok(out)
}

/// `[[code, value], …]` of one application as groups, each value checked for its code.
fn groups_from_json(app: &str, pairs: &[Value]) -> Result<Vec<RawTag>, String> {
    if pairs.len() > MAX_XDATA_GROUPS {
        return Err(format!("xdata of `{app}`: at most {MAX_XDATA_GROUPS} groups"));
    }
    let mut out = Vec::with_capacity(pairs.len());
    for (n, pair) in pairs.iter().enumerate() {
        let at = || format!("xdata of `{app}`, group {}", n + 1);
        let (code, value) = match pair.as_array().map(Vec::as_slice) {
            Some([c, v]) => (c.as_i64().and_then(|c| i32::try_from(c).ok()).ok_or_else(|| format!("{}: the code must be an integer", at()))?, v),
            _ => return Err(format!("{}: each group is a [code, value] pair", at())),
        };
        let real = |x: &Value| x.as_f64().filter(|x| x.is_finite()).map(cadcraft_dxf::format_real);
        match code {
            1000 | 1003 => {
                let s = value.as_str().ok_or_else(|| format!("{}: group {code} takes a string", at()))?;
                out.push(RawTag { code, value: s.to_string() });
            }
            1002 => match value.as_str() {
                Some(b @ ("{" | "}")) => out.push(RawTag { code, value: b.to_string() }),
                _ => return Err(format!("{}: group 1002 takes \"{{\" or \"}}\"", at())),
            },
            1004 => {
                let s = value.as_str().ok_or_else(|| format!("{}: group 1004 takes hex text", at()))?;
                out.push(RawTag { code, value: s.trim().to_ascii_uppercase() });
            }
            1005 => {
                let s = value.as_str().ok_or_else(|| format!("{}: group 1005 takes a hex handle", at()))?;
                out.push(RawTag { code, value: s.trim().to_ascii_uppercase() });
            }
            1010..=1013 => {
                let p = value.as_array().filter(|a| a.len() == 2 || a.len() == 3).and_then(|a| a.iter().map(real).collect::<Option<Vec<_>>>());
                let Some(p) = p else { return Err(format!("{}: group {code} takes a point [x, y, z] of finite numbers", at())) };
                out.push(RawTag { code, value: p.first().cloned().unwrap_or_default() });
                out.push(RawTag { code: code + 10, value: p.get(1).cloned().unwrap_or_default() });
                out.push(RawTag { code: code + 20, value: p.get(2).cloned().unwrap_or_else(|| "0.0".into()) });
            }
            1020..=1023 | 1030..=1033 => return Err(format!("{}: give the point as [{}, [x, y, z]]", at(), 1010 + (code % 10))),
            1040..=1042 => {
                let x = real(value).ok_or_else(|| format!("{}: group {code} takes a finite number", at()))?;
                out.push(RawTag { code, value: x });
            }
            1070 | 1071 => {
                let i = value.as_i64().ok_or_else(|| format!("{}: group {code} takes an integer", at()))?;
                out.push(RawTag { code, value: i.to_string() });
            }
            _ => return Err(format!("{}: {code} is not an xdata group code (1000–1071)", at())),
        }
    }
    check_xdata_groups(&out).map_err(|e| format!("xdata of `{app}`: {e}"))?;
    Ok(out)
}

/// Apply a parsed `xdata` parameter to an entity's xdata.
pub(crate) fn apply(xdata: &mut Vec<RawTag>, changes: &[(String, Option<Vec<RawTag>>)]) {
    for (app, groups) in changes {
        set_xdata_app(xdata, app, groups.as_deref());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn t(code: i32, value: &str) -> RawTag {
        RawTag { code, value: value.into() }
    }

    #[test]
    fn json_round_trip_types_every_kind() {
        let x = vec![
            t(1001, "ACME"),
            t(1000, "D-07"),
            t(1002, "{"),
            t(1003, "Walls"),
            t(1004, "0AFF"),
            t(1005, "2A"),
            t(1010, "1.5"),
            t(1020, "2.0"),
            t(1030, "0.0"),
            t(1040, "2.5"),
            t(1070, "3"),
            t(1071, "70000"),
            t(1002, "}"),
            t(1001, "PE_URL"),
            t(1000, "https://example.com"),
        ];
        let j = xdata_json(&x).unwrap();
        assert_eq!(
            j["ACME"],
            json!([
                [1000, "D-07"],
                [1002, "{"],
                [1003, "Walls"],
                [1004, "0AFF"],
                [1005, "2A"],
                [1010, [1.5, 2.0, 0.0]],
                [1040, 2.5],
                [1070, 3],
                [1071, 70000],
                [1002, "}"]
            ])
        );
        assert_eq!(j["PE_URL"], json!([[1000, "https://example.com"]]));
        let back = xdata_from_json(&j).unwrap();
        let mut y = Vec::new();
        apply(&mut y, &back);
        assert_eq!(y, x);
        assert_eq!(xdata_json(&[]), None);
        assert_eq!(XdataSelect::Apps(vec!["pe_url".into()]).json(&x).unwrap().as_object().unwrap().len(), 1);
        assert!(XdataSelect::Apps(vec!["nope".into()]).json(&x).is_none());
        assert!(!XdataSelect::Apps(vec!["nope".into()]).matches(&x) && XdataSelect::Apps(vec!["acme".into()]).matches(&x));
    }

    #[test]
    fn bad_input_is_refused() {
        for bad in [
            json!([]),
            json!({"": [[1000, "x"]]}),
            json!({"CADCRAFT": [[1000, "x"]]}),
            json!({"A": [[1000, 5]]}),
            json!({"A": [[1001, "B"]]}),
            json!({"A": [[8, "layer"]]}),
            json!({"A": [[1002, "("]]}),
            json!({"A": [[1002, "{"]]}),
            json!({"A": [[1010, [1, 2, 3, 4]]]}),
            json!({"A": [[1010, [1, "x"]]]}),
            json!({"A": [[1010, 1]]}),
            json!({"A": [[1020, 1]]}),
            json!({"A": [[1040, "1"]]}),
            json!({"A": [[1070, 1.5]]}),
            json!({"A": [[1070, 70000]]}),
            json!({"A": [[1071, 1e12]]}),
            json!({"A": [[1004, "ABC"]]}),
            json!({"A": [[1005, "xyz"]]}),
            json!({"A": [1000, "x"]}),
            json!({"A": "x"}),
            json!({"A": [[1000, "x".repeat(300)]]}),
        ] {
            assert!(xdata_from_json(&bad).is_err(), "{bad}");
        }
        assert_eq!(xdata_from_json(&json!({"A": null})).unwrap(), vec![("A".to_string(), None)]);
        assert_eq!(xdata_from_json(&json!({"A": []})).unwrap(), vec![("A".to_string(), Some(Vec::new()))]);
        assert!(XdataSelect::parse(Some(&json!(1))).is_err());
        assert!(XdataSelect::parse(Some(&json!([]))).is_err());
        assert!(XdataSelect::parse(Some(&json!([1]))).is_err());
        assert_eq!(XdataSelect::parse(Some(&json!(true))).unwrap(), XdataSelect::All);
        assert_eq!(XdataSelect::parse(Some(&json!("A"))).unwrap(), XdataSelect::Apps(vec!["A".into()]));
        assert_eq!(XdataSelect::parse(None).unwrap(), XdataSelect::None);
    }
}
