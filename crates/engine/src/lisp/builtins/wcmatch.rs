use super::args::{R, arity, string};
use super::{LispError, Value};

/// `wcmatch`: `#` a digit, `@` a letter, `.` neither, `*` any run, `?` any character, `~` (first)
/// not, `[…]` a set (`[~…]` not in it, ranges `a-z`), `` ` `` the next character as is, `,`
/// between alternative patterns. Letters compare case-sensitively.
pub(super) fn wcmatch(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (s, pat) = (string(&a[0])?, string(&a[1])?);
    let text: Vec<char> = s.chars().collect();
    // Native wildcard matching runs as a single interpreter step, so its internal `*` backtracking
    // must not explore exponentially many placements unchecked (CWE-400). Charge every glob step
    // against a shared budget and fail the call if it is exhausted.
    let mut budget: u64 = WCMATCH_BUDGET;
    for p in split_patterns(&pat, &mut budget).map_err(|()| LispError::new("wcmatch: pattern too complex"))? {
        match match_pattern(&text, &p, &mut budget) {
            Ok(true) => return Ok(Value::bool(true)),
            Ok(false) => {}
            Err(()) => return Err(LispError::new("wcmatch: pattern too complex")),
        }
    }
    Ok(Value::bool(false))
}

/// Shared step budget for one `wcmatch` call's wildcard matching. Large enough for the patterns
/// AutoLISP routines legitimately use (including the pathological-but-bounded cases the tests
/// cover), but finite so truly exponential `*`-heavy patterns are rejected rather than hanging.
const WCMATCH_BUDGET: u64 = 200_000_000;

/// Spend one wildcard work unit, returning an error when the shared budget is exhausted.
fn charge_match(budget: &mut u64) -> Result<(), ()> {
    *budget = budget.checked_sub(1).ok_or(())?;
    Ok(())
}

/// Split comma-separated wildcard alternatives while preserving escapes and bracket sets.
/// Every consumed character spends from the same budget used for matching.
fn split_patterns(pat: &str, budget: &mut u64) -> Result<Vec<Vec<char>>, ()> {
    let mut out = vec![Vec::new()];
    let mut chars = pat.chars().peekable();
    let mut in_set = false;
    while let Some(c) = chars.next() {
        charge_match(budget)?;
        match c {
            '`' => {
                if let Some(cur) = out.last_mut() {
                    cur.push('`');
                    if let Some(escaped) = chars.next() {
                        charge_match(budget)?;
                        cur.push(escaped);
                    }
                }
            }
            '[' => {
                in_set = true;
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
            ']' => {
                in_set = false;
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
            ',' if !in_set => out.push(Vec::new()),
            c => {
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
        }
    }
    Ok(out)
}

/// Match one wildcard alternative, including leading `~` negation, within the shared budget.
fn match_pattern(text: &[char], pat: &[char], budget: &mut u64) -> Result<bool, ()> {
    if let Some(('~', rest)) = pat.split_first().map(|(c, r)| (*c, r))
        && !rest.is_empty()
    {
        return glob(text, rest, 0, budget).map(|m| !m);
    }
    glob(text, pat, 0, budget)
}

/// Iterative-enough glob with `*` backtracking; `depth` bounds the recursion on `*`. Every step is
/// charged against `budget`; `Err(())` means the shared budget ran out (treated as too complex).
fn glob(text: &[char], pat: &[char], depth: usize, budget: &mut u64) -> Result<bool, ()> {
    glob_cached(text, pat, depth, budget, &mut std::collections::HashSet::new())
}

/// Match suffixes while memoizing failures; both matching work and retained failure entries are bounded.
fn glob_cached(
    text: &[char],
    pat: &[char],
    depth: usize,
    budget: &mut u64,
    failed: &mut std::collections::HashSet<(usize, usize, usize)>,
) -> Result<bool, ()> {
    charge_match(budget)?;
    let key = (text.len(), pat.len(), depth);
    if failed.contains(&key) {
        return Ok(false);
    }
    let matched = glob_inner(text, pat, depth, budget, failed)?;
    // Avoid revisiting the same failed suffix exponentially. Bound the memo table independently
    // of the step limit so a very large pattern cannot turn the optimization into an allocation bomb.
    if !matched && failed.len() < 100_000 {
        failed.insert(key);
    }
    Ok(matched)
}

/// For the bracket class opening at `pat[p]`, whether `text[t]` is in it: the index of its closing
/// `]` when it is, `None` when it is not or the class is never closed.
fn class_end(text: &[char], pat: &[char], p: usize, t: usize, budget: &mut u64) -> Result<Option<usize>, ()> {
    let mut close = p;
    while let Some(c) = pat.get(close) {
        charge_match(budget)?;
        if *c == ']' {
            break;
        }
        close += 1;
    }
    if close == pat.len() {
        return Ok(None);
    }
    let Some(&c) = text.get(t) else { return Ok(None) };
    let set = &pat[p + 1..close];
    let (neg, set) = match set.split_first() {
        Some(('~', r)) if !r.is_empty() => (true, r),
        _ => (false, set),
    };
    let mut hit = false;
    let mut i = 0;
    while i < set.len() {
        charge_match(budget)?;
        if set.get(i + 1) == Some(&'-') && i + 2 < set.len() {
            if set[i] <= c && c <= set[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if set[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    if hit == neg {
        return Ok(None);
    }
    Ok(Some(close))
}

/// Evaluate wildcard tokens and backtracking branches under shared work and recursion limits.
fn glob_inner(
    text: &[char],
    pat: &[char],
    depth: usize,
    budget: &mut u64,
    failed: &mut std::collections::HashSet<(usize, usize, usize)>,
) -> Result<bool, ()> {
    if *budget == 0 {
        return Err(());
    }
    *budget -= 1;
    if depth > 256 {
        return Err(());
    }
    let (mut t, mut p) = (0usize, 0usize);
    while p < pat.len() {
        charge_match(budget)?;
        match pat[p] {
            '*' => {
                let rest = &pat[p + 1..];
                for i in t..=text.len() {
                    if glob_cached(&text[i..], rest, depth + 1, budget, failed)? {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
            '`' => {
                let Some(&lit) = pat.get(p + 1) else { return Ok(false) };
                if text.get(t) != Some(&lit) {
                    return Ok(false);
                }
                t += 1;
                p += 2;
            }
            '[' => {
                let Some(close) = class_end(text, pat, p, t, budget)? else { return Ok(false) };
                t += 1;
                p = close + 1;
            }
            pc => {
                let Some(&c) = text.get(t) else { return Ok(false) };
                let ok = match pc {
                    '#' => c.is_ascii_digit(),
                    '@' => c.is_alphabetic(),
                    '.' => !c.is_alphanumeric(),
                    '?' => true,
                    other => other == c,
                };
                if !ok {
                    return Ok(false);
                }
                t += 1;
                p += 1;
            }
        }
    }
    Ok(t == text.len())
}

/// `wcmatch` for other callers (`ssget` filters).
pub fn wild_match(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let mut budget: u64 = WCMATCH_BUDGET;
    split_patterns(pattern, &mut budget).is_ok_and(|patterns| patterns.iter().any(|p| match_pattern(&t, p, &mut budget) == Ok(true)))
}

#[cfg(test)]
mod wildcard_budget_tests {
    use super::*;

    /// Pattern splitting, literal matching, and bracket scans all stop when their shared fuel runs out.
    #[test]
    fn all_native_scans_consume_fuel() {
        assert!(split_patterns("abcdefgh", &mut 4).is_err());
        assert!(split_patterns("`a`b`c", &mut 4).is_err());
        let text: Vec<_> = "abcdefgh".chars().collect();
        assert!(glob(&text, &text, 0, &mut 4).is_err());
        let pattern: Vec<_> = "[abcdefgh]".chars().collect();
        assert!(glob(&['!'], &pattern, 0, &mut 6).is_err());
        // Enough for finding the closing bracket, but not scanning the entire set again.
        assert!(glob(&['!'], &pattern, 0, &mut 14).is_err());
        assert_eq!(glob(&['a'], &pattern, 0, &mut 100), Ok(true));
    }
}
