use super::args::{R, arity, string};
use super::{LispError, Value};

// ---------------- the Windows registry ----------------
//
// `vl-registry-read`, `-write` and `-delete`. Windows only: elsewhere (and on the web) every call
// returns nil. A failure (missing key, no permission, a type we do not map) is nil, as in AutoLISP;
// only a wrong argument type is an error.

/// Longest key path or value name accepted, in characters (the registry's own limits are 255 per
/// key name and 16383 per value name).
#[cfg(windows)]
const MAX_NAME: usize = 16_383;

/// `(vl-registry-read regkey [valname])`: a string (REG_SZ, REG_EXPAND_SZ), an integer (REG_DWORD) or nil.
pub(super) fn vl_registry_read(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let key = string(&a[0])?;
    let name = optional_name(a.get(1))?;
    Ok(imp::read(&key, &name))
}

/// `(vl-registry-write regkey [valname] value)`: creates the key as needed; returns the value or nil.
pub(super) fn vl_registry_write(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let key = string(&a[0])?;
    let (name, value) = match a {
        [_, value] => (Default::default(), value),
        [_, name, value] => (optional_name(Some(name))?, value),
        _ => return Err(super::args_err(true)),
    };
    if !matches!(value, Value::Str(_) | Value::Int(_)) {
        return Err(LispError::bad_arg("stringp", value));
    }
    Ok(if imp::write(&key, &name, value) { value.clone() } else { Value::Nil })
}

/// `(vl-registry-delete regkey [valname])`: T when deleted. Without a value name, deletes the key
/// (only when it has no subkeys).
pub(super) fn vl_registry_delete(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let key = string(&a[0])?;
    let name = a.get(1).filter(|v| !matches!(v, Value::Nil)).map(string).transpose()?;
    Ok(if imp::delete(&key, name.as_deref()) { Value::sym("T") } else { Value::Nil })
}

/// A value name; absent or nil names the key's default value.
fn optional_name(v: Option<&Value>) -> Result<std::sync::Arc<str>, LispError> {
    Ok(v.filter(|v| !matches!(v, Value::Nil)).map(string).transpose()?.unwrap_or_default())
}

#[cfg(windows)]
mod imp {
    use super::{MAX_NAME, Value};
    use crate::lisp::value::MAX_STRING;
    use winreg::enums::{
        HKEY_CLASSES_ROOT, HKEY_CURRENT_CONFIG, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ,
        REG_SZ,
    };
    use winreg::{RegKey, RegValue};

    /// Split `HKEY_CURRENT_USER\Software\X` into its root and the path below it (which may be empty).
    fn split(key: &str) -> Option<(RegKey, &str)> {
        if key.chars().count() > MAX_NAME {
            return None;
        }
        let key = key.trim_matches('\\');
        let (root, rest) = key.split_once('\\').unwrap_or((key, ""));
        let hive = match root.to_ascii_uppercase().as_str() {
            "HKEY_CURRENT_USER" | "HKCU" => HKEY_CURRENT_USER,
            "HKEY_LOCAL_MACHINE" | "HKLM" => HKEY_LOCAL_MACHINE,
            "HKEY_CLASSES_ROOT" | "HKCR" => HKEY_CLASSES_ROOT,
            "HKEY_USERS" => HKEY_USERS,
            "HKEY_CURRENT_CONFIG" | "HKCC" => HKEY_CURRENT_CONFIG,
            _ => return None,
        };
        Some((RegKey::predef(hive), rest.trim_matches('\\')))
    }

    pub(super) fn read(key: &str, name: &str) -> Value {
        read_value(key, name).unwrap_or(Value::Nil)
    }

    fn read_value(key: &str, name: &str) -> Option<Value> {
        if name.chars().count() > MAX_NAME {
            return None;
        }
        let (root, path) = split(key)?;
        let k = if path.is_empty() { root } else { root.open_subkey_with_flags(path, KEY_READ).ok()? };
        let raw: RegValue = k.get_raw_value(name).ok()?;
        match raw.vtype {
            REG_DWORD => {
                let bytes: [u8; 4] = raw.bytes.as_slice().try_into().ok()?;
                Some(Value::Int(i32::from_ne_bytes(u32::from_ne_bytes(bytes).to_ne_bytes())))
            }
            REG_SZ | REG_EXPAND_SZ => {
                // Bound the text before decoding: UTF-16 is two bytes per unit.
                if raw.bytes.len() / 2 > MAX_STRING {
                    return None;
                }
                let units: Vec<u16> = raw.bytes.chunks(2).filter_map(|c| <[u8; 2]>::try_from(c).ok()).map(u16::from_le_bytes).collect();
                let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
                let s = String::from_utf16_lossy(units.get(..end)?);
                (s.len() <= MAX_STRING).then(|| Value::str(&s))
            }
            _ => None,
        }
    }

    pub(super) fn write(key: &str, name: &str, value: &Value) -> bool {
        if name.chars().count() > MAX_NAME {
            return false;
        }
        let Some((root, path)) = split(key) else { return false };
        let k = if path.is_empty() {
            root
        } else {
            match root.create_subkey_with_flags(path, KEY_SET_VALUE) {
                Ok((k, _)) => k,
                Err(_) => return false,
            }
        };
        match value {
            Value::Str(s) => k.set_value(name, &s.to_string()).is_ok(),
            Value::Int(i) => k.set_value(name, &u32::from_ne_bytes(i.to_ne_bytes())).is_ok(),
            _ => false,
        }
    }

    pub(super) fn delete(key: &str, name: Option<&str>) -> bool {
        let Some((root, path)) = split(key) else { return false };
        match name {
            Some(n) => {
                if path.is_empty() || n.chars().count() > MAX_NAME {
                    return false;
                }
                root.open_subkey_with_flags(path, KEY_SET_VALUE).and_then(|k| k.delete_value(n)).is_ok()
            }
            None => {
                // RegDeleteKey refuses a key that still has subkeys.
                let Some((parent, leaf)) = path.rsplit_once('\\').map_or(Some(("", path)), Some).filter(|(_, l)| !l.is_empty()) else {
                    return false;
                };
                let p = if parent.is_empty() {
                    root
                } else {
                    match root.open_subkey_with_flags(parent, KEY_READ | KEY_SET_VALUE) {
                        Ok(k) => k,
                        Err(_) => return false,
                    }
                };
                p.delete_subkey(leaf).is_ok()
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Value;

    pub(super) fn read(_key: &str, _name: &str) -> Value {
        Value::Nil
    }
    pub(super) fn write(_key: &str, _name: &str, _value: &Value) -> bool {
        false
    }
    pub(super) fn delete(_key: &str, _name: Option<&str>) -> bool {
        false
    }
}
