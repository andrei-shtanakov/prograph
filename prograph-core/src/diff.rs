//! Diff engine — compares new entities against the alive set in storage
//! and produces ChangeEvents (added / removed / attrs_changed).

use std::collections::HashMap;

/// A simplified change record produced by `compute_*` functions.
/// The indexer's persist phase emits the actual `change_log` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub identity_key: String,
    pub change: DiffChange,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffChange {
    Added,
    Removed,
    AttrsChanged,
    Unchanged, // not written to change_log but signalled so indexer can extend last_seen
}

/// Diff two sets of entities keyed by identity string.
///
/// `old` is the alive set from the previous snapshot (identity -> attrs_json).
/// `new` is the candidate set from the current scan (identity -> attrs_json).
///
/// For each identity:
/// - in new only            -> Added
/// - in old only            -> Removed
/// - in both, attrs equal   -> Unchanged
/// - in both, attrs differ  -> AttrsChanged
pub fn diff_by_identity(
    old: &HashMap<String, String>,
    new: &HashMap<String, String>,
) -> Vec<DiffEntry> {
    let mut out = Vec::new();

    for (key, new_attrs) in new {
        match old.get(key) {
            None => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Added,
                before_json: None,
                after_json: Some(new_attrs.clone()),
            }),
            Some(old_attrs) if old_attrs == new_attrs => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Unchanged,
                before_json: Some(old_attrs.clone()),
                after_json: Some(new_attrs.clone()),
            }),
            Some(old_attrs) => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::AttrsChanged,
                before_json: Some(old_attrs.clone()),
                after_json: Some(new_attrs.clone()),
            }),
        }
    }

    for (key, old_attrs) in old {
        if !new.contains_key(key) {
            out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Removed,
                before_json: Some(old_attrs.clone()),
                after_json: None,
            });
        }
    }

    out.sort_by(|a, b| a.identity_key.cmp(&b.identity_key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn hm(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detects_added() {
        let old = hm(&[]);
        let new = hm(&[("a", "{}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Added);
    }

    #[test]
    fn detects_removed() {
        let old = hm(&[("a", "{}")]);
        let new = hm(&[]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Removed);
    }

    #[test]
    fn detects_attrs_changed() {
        let old = hm(&[("a", "{\"v\":1}")]);
        let new = hm(&[("a", "{\"v\":2}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::AttrsChanged);
        assert_eq!(d[0].before_json.as_deref(), Some("{\"v\":1}"));
        assert_eq!(d[0].after_json.as_deref(), Some("{\"v\":2}"));
    }

    #[test]
    fn detects_unchanged() {
        let old = hm(&[("a", "{}")]);
        let new = hm(&[("a", "{}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Unchanged);
    }

    #[test]
    fn mixed_diff_is_sorted_by_key() {
        let old = hm(&[("a", "1"), ("b", "1"), ("d", "1")]);
        let new = hm(&[("a", "1"), ("b", "2"), ("c", "1")]);
        let d = diff_by_identity(&old, &new);
        let keys: Vec<_> = d.iter().map(|e| e.identity_key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c", "d"]);
        let changes: Vec<_> = d.iter().map(|e| e.change).collect();
        assert_eq!(
            changes,
            vec![
                DiffChange::Unchanged,
                DiffChange::AttrsChanged,
                DiffChange::Added,
                DiffChange::Removed
            ]
        );
    }

    #[test]
    fn empty_both_sides_returns_empty() {
        let d = diff_by_identity(&hm(&[]), &hm(&[]));
        assert!(d.is_empty());
    }
}
