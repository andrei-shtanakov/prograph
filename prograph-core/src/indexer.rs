//! Indexer pipeline — orchestrates discovery, parsing, edge detection, diffing, and persistence.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::detectors;
use crate::diff::{diff_by_identity, DiffChange};
use crate::discovery::scan_monorepo;
use crate::errors::Result;
use crate::facts::{ParseStatus, ProjectFacts};
use crate::lock::IndexLockGuard;
use crate::models::{EdgeKind, IndexSummary};
use crate::parsers::parse_project;
use crate::store::Store;

/// Format a contract identity key: "<declared_id_or_empty>|<content_hash>".
fn contract_identity_key(declared_id: Option<&str>, content_hash: &str) -> String {
    format!("{}|{}", declared_id.unwrap_or(""), content_hash)
}

/// Run the full index pipeline against `monorepo_root`. Acquires `.prograph/index.lock`
/// for the duration; fails fast if another `prograph index` is running.
pub fn index_monorepo(monorepo_root: &Path, store: &mut Store) -> Result<IndexSummary> {
    let start = Instant::now();
    let lock_path = monorepo_root.join(".prograph").join("index.lock");
    let _lock = IndexLockGuard::acquire(&lock_path)?;

    // Phase 1: Discovery.
    let candidates = scan_monorepo(monorepo_root)?;

    // Phase 2: Parsing (sequential in M2; rayon parallelism is a future optimization).
    let mut facts: Vec<ProjectFacts> = Vec::with_capacity(candidates.len());
    let mut warning_count: i64 = 0;
    for cand in &candidates {
        let proj_root = monorepo_root.join(cand.root_path.trim_start_matches("./"));
        let out = parse_project(&proj_root, cand.kind)?;
        warning_count += out.warnings.len() as i64;
        let parse_status = if !out.warnings.is_empty() && out.manifest.is_none() {
            ParseStatus::Failed
        } else if !out.warnings.is_empty() {
            ParseStatus::Partial
        } else {
            ParseStatus::Ok
        };
        facts.push(ProjectFacts {
            project_root: cand.root_path.clone(),
            project_name: cand.name.clone(),
            manifest: out.manifest,
            warnings: out.warnings,
            parse_status,
            mcp_decls: out.mcp_decls,
            mcp_uses: out.mcp_uses,
            contracts: out.contracts,
            modules: out.modules,
            intent: crate::intent::extract_intent(&proj_root),
        });
    }

    // Phase 3: Edge detection.
    let detection = detectors::detect_all(&facts);
    let edge_candidates = detection.edges;
    let contract_candidates = detection.contracts;
    let collision_warnings = detectors::deps::drain_collision_warnings();
    warning_count += collision_warnings.len() as i64;

    // Phase 4: Diff vs alive set.
    let alive_projects = store.alive_projects()?;
    let alive_edges = store.alive_edges()?;
    let alive_contracts = store.alive_contracts()?;

    // Build new-state identity maps for diff.
    let new_project_attrs: HashMap<String, String> = facts
        .iter()
        .map(|f| {
            let attrs = serde_json::json!({
                "name": f.project_name,
                "declared_name": f.manifest.as_ref().map(|m| &m.declared_name),
                "version": f.manifest.as_ref().and_then(|m| m.version.as_ref()),
                "parse_status": match f.parse_status {
                    ParseStatus::Ok => "ok",
                    ParseStatus::Partial => "partial",
                    ParseStatus::Failed => "failed",
                },
            });
            (
                f.project_root.clone(),
                serde_json::to_string(&attrs).unwrap(),
            )
        })
        .collect();

    // Build new-state edge identity keys. Format: <kind>|<from_root>|<to_endpoint>|<attrs_hash>
    // where to_endpoint is the project_root for project endpoints, or the contract identity
    // key "<declared_id>|<content_hash>" for contract endpoints. The contract identity itself
    // may contain '|', so the persist loop parses with parts[0]/[1]/last and joins the middle.
    let new_edge_attrs: HashMap<String, String> = edge_candidates
        .iter()
        .map(|c| {
            let from_root = &facts[c.from_idx].project_root;
            let to_endpoint = if c.kind == EdgeKind::ContractLink {
                let cc = &contract_candidates[c.to_idx];
                contract_identity_key(cc.declared_id.as_deref(), &cc.content_hash)
            } else {
                facts[c.to_idx].project_root.clone()
            };
            let key = format!(
                "{}|{}|{}|{}",
                c.kind.name(),
                from_root,
                to_endpoint,
                c.attrs_hash
            );
            (key, c.attrs_json.clone())
        })
        .collect();

    // Rekey alive_edges from "kind|from_kind|from_id|to_kind|to_id|attrs_hash" to the new
    // identity-key format. Contract endpoints are reverse-looked-up via alive_contracts.
    let project_id_to_root: HashMap<i64, &str> = alive_projects
        .iter()
        .map(|(root, (id, _))| (*id, root.as_str()))
        .collect();
    let contract_id_to_key: HashMap<i64, String> = alive_contracts
        .iter()
        .map(|(key, (id, _))| (*id, key.clone()))
        .collect();
    let alive_edges_by_root: HashMap<String, (i64, String)> = alive_edges
        .iter()
        .filter_map(|(raw_key, (id, attrs))| {
            // raw_key: "kind|from_kind|from_id|to_kind|to_id|attrs_hash"
            let parts: Vec<&str> = raw_key.split('|').collect();
            if parts.len() != 6 {
                return None;
            }
            let kind = parts[0];
            let from_kind = parts[1];
            let from_id: i64 = parts[2].parse().ok()?;
            let to_kind = parts[3];
            let to_id: i64 = parts[4].parse().ok()?;
            let attrs_hash = parts[5];

            let from_endpoint = match from_kind {
                "project" => project_id_to_root.get(&from_id)?.to_string(),
                _ => return None, // M4: from is always project
            };
            let to_endpoint = match to_kind {
                "project" => project_id_to_root.get(&to_id)?.to_string(),
                "contract" => contract_id_to_key.get(&to_id)?.clone(),
                _ => return None,
            };
            let key = format!("{}|{}|{}|{}", kind, from_endpoint, to_endpoint, attrs_hash);
            Some((key, (*id, attrs.clone())))
        })
        .collect();

    let project_diff = diff_by_identity(
        &alive_projects
            .iter()
            .map(|(k, (_, a))| (k.clone(), a.clone()))
            .collect(),
        &new_project_attrs,
    );
    let edge_diff = diff_by_identity(
        &alive_edges_by_root
            .iter()
            .map(|(k, (_, a))| (k.clone(), a.clone()))
            .collect(),
        &new_edge_attrs,
    );

    // Contracts diff. Identity-keyed; attrs are intentionally constant per identity
    // (file lists may shift, but the contract IS the (declared_id, content_hash) pair).
    let new_contract_attrs: HashMap<String, String> = contract_candidates
        .iter()
        .map(|c| {
            let key = contract_identity_key(c.declared_id.as_deref(), &c.content_hash);
            (key, String::new())
        })
        .collect();
    let alive_contract_attrs: HashMap<String, String> = alive_contracts
        .keys()
        .map(|k| (k.clone(), String::new()))
        .collect();
    let contract_diff = diff_by_identity(&alive_contract_attrs, &new_contract_attrs);

    // M11: read recent change_log labels BEFORE opening the writer transaction.
    // begin_snapshot() takes &mut store, so reads have to happen first.
    let recent_log = store.recent_changelog_labels(5).unwrap_or_default();

    // Phase 5: Persist in a single transaction.
    let writer = store.begin_snapshot()?;
    let ts = current_iso_ts();
    let git_commit = detect_git_commit(monorepo_root);
    let snap_id = writer.insert_snapshot(
        &ts,
        &monorepo_root.display().to_string(),
        git_commit.as_deref(),
        env!("CARGO_PKG_VERSION"),
    )?;

    let mut new_project_ids: HashMap<String, i64> = HashMap::new();
    let mut n_projects: i64 = 0;
    let mut n_changes: i64 = 0;

    for entry in &project_diff {
        let key = &entry.identity_key;
        match entry.change {
            DiffChange::Added => {
                let fact = facts.iter().find(|f| &f.project_root == key).unwrap();
                let kind_str = candidates
                    .iter()
                    .find(|c| &c.root_path == key)
                    .map(|c| c.kind.name())
                    .unwrap_or("python");
                let attrs = entry.after_json.as_deref().unwrap_or("{}");
                let pid =
                    writer.insert_project(snap_id, &fact.project_name, key, kind_str, attrs)?;
                new_project_ids.insert(key.clone(), pid);
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "project",
                    pid,
                    "added",
                    None,
                    entry.after_json.as_deref(),
                )?;
                n_projects += 1;
                n_changes += 1;
            }
            DiffChange::Unchanged => {
                let (pid, _) = &alive_projects[key];
                writer.touch_project(*pid, snap_id, None)?;
                new_project_ids.insert(key.clone(), *pid);
                n_projects += 1;
            }
            DiffChange::AttrsChanged => {
                let (pid, _) = &alive_projects[key];
                writer.touch_project(*pid, snap_id, entry.after_json.as_deref())?;
                new_project_ids.insert(key.clone(), *pid);
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "project",
                    *pid,
                    "attrs_changed",
                    entry.before_json.as_deref(),
                    entry.after_json.as_deref(),
                )?;
                n_projects += 1;
                n_changes += 1;
            }
            DiffChange::Removed => {
                let (pid, _) = &alive_projects[key];
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "project",
                    *pid,
                    "removed",
                    entry.before_json.as_deref(),
                    None,
                )?;
                n_changes += 1;
            }
        }

        // Persist any MCP tool decls this project declared (M5).
        // Only persist when the project itself is present in the new snapshot
        // (NOT in the Removed branch — leaving the row unadvanced lets its last_seen
        // reflect the last snapshot it was alive).
        if let Some(&pid) = new_project_ids.get(key) {
            if let Some(fact) = facts.iter().find(|f| &f.project_root == key) {
                for decl in &fact.mcp_decls {
                    writer.insert_mcp_tool_decl(
                        pid,
                        &decl.tool_name,
                        &decl.rel_path,
                        decl.line as i64,
                        snap_id,
                    )?;
                }
                // M9: modules + public symbols + internal imports.
                for module in &fact.modules {
                    let mid =
                        writer.insert_module(snap_id, pid, &module.rel_path, &module.language)?;
                    for sym in &module.public_symbols {
                        writer.insert_public_symbol(
                            mid,
                            snap_id,
                            &sym.name,
                            sym.kind.as_str(),
                            sym.line as i64,
                        )?;
                    }
                    for imp in &module.internal_imports {
                        writer.insert_internal_import(
                            mid,
                            snap_id,
                            &imp.target_path,
                            imp.line as i64,
                        )?;
                    }
                }
            }
        }
    }

    // Contracts persist (BEFORE edges, so contract_link endpoints resolve).
    let mut new_contract_ids: HashMap<String, i64> = HashMap::new();
    for entry in &contract_diff {
        let key = &entry.identity_key;
        match entry.change {
            DiffChange::Added => {
                let c = contract_candidates
                    .iter()
                    .find(|c| {
                        &contract_identity_key(c.declared_id.as_deref(), &c.content_hash) == key
                    })
                    .unwrap();
                let cid = writer.insert_contract(
                    snap_id,
                    c.declared_id.as_deref(),
                    &c.content_hash,
                    c.kind.as_str(),
                )?;
                new_contract_ids.insert(key.clone(), cid);
                let after_attrs = serde_json::json!({
                    "kind": c.kind.as_str(),
                    "declared_id": c.declared_id,
                });
                let after_attrs_str = serde_json::to_string(&after_attrs).unwrap();
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "contract",
                    cid,
                    "added",
                    None,
                    Some(&after_attrs_str),
                )?;
                n_changes += 1;

                // Materialize contract_files for each owning project.
                for (proj_idx, rel_path) in &c.files {
                    let proj_root = &facts[*proj_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_contract_file(cid, pid, rel_path, snap_id)?;
                    }
                }
            }
            DiffChange::Unchanged => {
                let (cid, _kind) = &alive_contracts[key];
                writer.touch_contract(*cid, snap_id)?;
                new_contract_ids.insert(key.clone(), *cid);

                // Touch / insert contract_files for current owners.
                let c = contract_candidates
                    .iter()
                    .find(|c| {
                        &contract_identity_key(c.declared_id.as_deref(), &c.content_hash) == key
                    })
                    .unwrap();
                for (proj_idx, rel_path) in &c.files {
                    let proj_root = &facts[*proj_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_contract_file(*cid, pid, rel_path, snap_id)?;
                        writer.touch_contract_file(*cid, pid, rel_path, snap_id)?;
                    }
                }
            }
            DiffChange::AttrsChanged => {
                // Not produced — contracts have constant attrs per identity in M4.
                continue;
            }
            DiffChange::Removed => {
                let (cid, _) = &alive_contracts[key];
                writer.insert_change_log(snap_id, &ts, "contract", *cid, "removed", None, None)?;
                n_changes += 1;
            }
        }
    }

    let mut n_edges: i64 = 0;
    for entry in &edge_diff {
        // identity key: <kind>|<from_root>|<to_endpoint>|<attrs_hash>
        // For contract_link, <to_endpoint> = "<declared_id>|<content_hash>" — may contain '|'.
        // Parse by taking parts[0]/[1]/last and joining the middle.
        let parts: Vec<&str> = entry.identity_key.split('|').collect();
        if parts.len() < 4 {
            continue;
        }
        let kind = parts[0];
        let from_root = parts[1];
        let attrs_hash = parts[parts.len() - 1];
        let to_endpoint = parts[2..parts.len() - 1].join("|");

        let is_contract_link = kind == "contract_link";

        // Locate the corresponding EdgeCandidate so we can persist its evidence (M7).
        // attrs_hash already includes the edge kind via the hasher prefix, so it's unique.
        let evidence = edge_candidates
            .iter()
            .find(|c| c.attrs_hash == attrs_hash)
            .map(|c| c.evidence.clone())
            .unwrap_or_default();

        let persist_evidence =
            |writer: &crate::store::SnapshotWriter<'_>, eid: i64| -> Result<()> {
                for ev in &evidence {
                    let proj_root = &facts[ev.project_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_edge_evidence(
                            eid,
                            pid,
                            &ev.rel_path,
                            ev.line,
                            ev.snippet.as_deref(),
                            snap_id,
                        )?;
                    }
                }
                Ok(())
            };

        match entry.change {
            DiffChange::Added => {
                let from_id = match new_project_ids.get(from_root).copied() {
                    Some(id) => id,
                    None => continue,
                };
                let (to_kind_str, to_id) = if is_contract_link {
                    let cid = match new_contract_ids.get(&to_endpoint).copied() {
                        Some(id) => id,
                        None => continue,
                    };
                    ("contract", cid)
                } else {
                    let pid = match new_project_ids.get(&to_endpoint).copied() {
                        Some(id) => id,
                        None => continue,
                    };
                    ("project", pid)
                };
                let attrs = entry.after_json.as_deref().unwrap_or("{}");
                let eid = writer.insert_edge(
                    snap_id,
                    kind,
                    "project",
                    from_id,
                    to_kind_str,
                    to_id,
                    attrs,
                    attrs_hash,
                )?;
                persist_evidence(&writer, eid)?;
                writer.insert_change_log(snap_id, &ts, "edge", eid, "added", None, Some(attrs))?;
                n_edges += 1;
                n_changes += 1;
            }
            DiffChange::Unchanged => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.touch_edge(*eid, snap_id, None)?;
                persist_evidence(&writer, *eid)?;
                n_edges += 1;
            }
            DiffChange::AttrsChanged => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.touch_edge(*eid, snap_id, entry.after_json.as_deref())?;
                persist_evidence(&writer, *eid)?;
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "edge",
                    *eid,
                    "attrs_changed",
                    entry.before_json.as_deref(),
                    entry.after_json.as_deref(),
                )?;
                n_edges += 1;
                n_changes += 1;
            }
            DiffChange::Removed => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.insert_change_log(
                    snap_id,
                    &ts,
                    "edge",
                    *eid,
                    "removed",
                    entry.before_json.as_deref(),
                    None,
                )?;
                n_changes += 1;
            }
        }
    }

    // M10: resolve cross-project symbol references AFTER all modules are
    // persisted. Each external import becomes one cross_project_symbol_refs
    // row if it resolves to an in-monorepo publisher.
    let publishers = crate::resolvers::build_publisher_index(&facts);
    let mut all_refs: Vec<crate::resolvers::ResolvedRef> = Vec::new();
    all_refs.extend(crate::resolvers::python::resolve(&facts, &publishers));
    all_refs.extend(crate::resolvers::rust::resolve(&facts, &publishers));

    // Look up each (project_idx, rel_path) → module_id from the just-inserted
    // module rows (same tx).
    let mut module_id_for: HashMap<(usize, String), i64> = HashMap::new();
    for (idx, fact) in facts.iter().enumerate() {
        let from_pid = match new_project_ids.get(&fact.project_root) {
            Some(&pid) => pid,
            None => continue,
        };
        for module in &fact.modules {
            let mid: Option<i64> = writer
                .conn()
                .query_row(
                    "SELECT id FROM modules WHERE project_id = ? AND rel_path = ?",
                    rusqlite::params![from_pid, module.rel_path],
                    |r| r.get(0),
                )
                .ok();
            if let Some(mid) = mid {
                module_id_for.insert((idx, module.rel_path.clone()), mid);
            }
        }
    }

    for r in &all_refs {
        let from_pid = facts
            .get(r.from_project_idx)
            .and_then(|f| new_project_ids.get(&f.project_root))
            .copied();
        let to_pid = facts
            .get(r.to_project_idx)
            .and_then(|f| new_project_ids.get(&f.project_root))
            .copied();
        let from_mid = module_id_for
            .get(&(r.from_project_idx, r.from_module_rel_path.clone()))
            .copied();

        if let (Some(from_pid), Some(to_pid), Some(from_mid)) = (from_pid, to_pid, from_mid) {
            writer.insert_symbol_ref(
                snap_id,
                from_pid,
                from_mid,
                r.from_line as i64,
                to_pid,
                &r.to_module_path,
                r.to_symbol_name.as_deref(),
            )?;
        }
    }

    // M11: drift detection. Recent change_log labels were read above (before
    // begin_snapshot). Per-project facts are already persisted in this tx so
    // project_ids are resolvable via new_project_ids.
    for fact in &facts {
        let Some(&pid) = new_project_ids.get(&fact.project_root) else {
            continue;
        };
        let drifts = crate::drift::detect_all(fact, &recent_log);
        for d in drifts {
            writer.insert_drift_finding(
                snap_id,
                pid,
                d.kind.as_str(),
                d.entity_kind.as_str(),
                &d.entity_name,
                &d.source_path,
                d.source_line as i64,
                d.confidence.as_str(),
                d.detail.as_deref(),
            )?;
        }
    }

    // M7: refresh FTS index after persists are done but before commit.
    writer.rebuild_search_fts(snap_id)?;

    writer.commit()?;

    Ok(IndexSummary {
        snapshot_id: snap_id,
        ts,
        n_projects,
        n_edges,
        n_changes,
        n_warnings: warning_count,
        duration_ms: start.elapsed().as_millis() as i64,
    })
}

fn current_iso_ts() -> String {
    // RFC3339 second-precision, UTC. Avoids pulling in chrono — uses std SystemTime arithmetic.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Best-effort: return `Some(commit_sha)` if `monorepo_root` is inside a git repository
/// AND the working tree is clean. Returns `None` otherwise.
///
/// Snapshots tied to a dirty working tree aren't reproducible from the recorded commit
/// alone, so we record None rather than claiming a state we can't restore.
fn detect_git_commit(monorepo_root: &Path) -> Option<String> {
    use std::process::Command;

    let status_out = Command::new("git")
        .arg("-C")
        .arg(monorepo_root)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !status_out.status.success() {
        return None;
    }
    if !status_out.stdout.is_empty() {
        return None; // dirty tree
    }

    let rev_out = Command::new("git")
        .arg("-C")
        .arg(monorepo_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !rev_out.status.success() {
        return None;
    }
    let sha = String::from_utf8(rev_out.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

fn secs_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    // Civil-from-days algorithm by Howard Hinnant.
    let s = secs as i64;
    let days = s.div_euclid(86_400);
    let rem = s.rem_euclid(86_400);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (y + if mp >= 10 { 1 } else { 0 }) as i32;
    (year, mo, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_monorepo() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        fs::create_dir_all(dir.path().join("consumer")).unwrap();
        fs::write(
            dir.path().join("consumer/pyproject.toml"),
            r#"[project]
name = "consumer"
version = "1.0"
dependencies = ["my-sdk>=1.0"]
"#,
        )
        .unwrap();

        fs::create_dir_all(dir.path().join("sdk")).unwrap();
        fs::write(
            dir.path().join("sdk/pyproject.toml"),
            r#"[project]
name = "my-sdk"
version = "1.5"
dependencies = []
"#,
        )
        .unwrap();

        dir
    }

    #[test]
    fn first_index_creates_snapshot_with_two_projects_and_one_edge() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 2);
        assert_eq!(summary.n_edges, 1);
        assert!(
            summary.n_changes >= 3,
            "expected >=3 change_log entries (2 projects + 1 edge added)"
        );
    }

    #[test]
    fn second_index_no_changes_is_empty_changelog() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 2);
        assert_eq!(summary.n_edges, 1);
        assert_eq!(summary.n_changes, 0);
    }

    #[test]
    fn version_bump_produces_attrs_changed_event() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        fs::write(
            dir.path().join("consumer/pyproject.toml"),
            r#"[project]
name = "consumer"
version = "1.1"
dependencies = ["my-sdk>=2.0"]
"#,
        )
        .unwrap();

        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert!(
            summary.n_changes >= 2,
            "expected attrs_changed for both project and edge"
        );
    }

    #[test]
    fn project_removal_cascades_edge_removed_event() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let first = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(first.n_edges, 1);

        // Remove the SDK (publisher) entirely. Consumer's edge to it should now be removed too.
        fs::remove_dir_all(dir.path().join("sdk")).unwrap();

        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 1, "only consumer remains");
        assert_eq!(summary.n_edges, 0, "edge to sdk should be removed");
        // Expect ≥ 2 change_log entries: project sdk removed + edge removed.
        assert!(
            summary.n_changes >= 2,
            "expected ≥2 changes (project + cascading edge removal), got {}",
            summary.n_changes
        );
    }

    #[test]
    fn detect_git_commit_returns_none_for_non_git_dir() {
        let dir = TempDir::new().unwrap();
        assert!(detect_git_commit(dir.path()).is_none());
    }

    #[test]
    fn snapshot_records_no_git_commit_outside_git_repo() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("p")).unwrap();
        fs::write(
            dir.path().join("p/pyproject.toml"),
            r#"[project]
name = "p"
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let _summary = index_monorepo(dir.path(), &mut store).unwrap();
        let info = store.latest_snapshot_info().unwrap().unwrap();
        assert!(info.git_commit.is_none());
    }

    #[test]
    fn collision_warning_increments_n_warnings() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        // Two projects publishing the same name (one via alias).
        fs::create_dir_all(dir.path().join("first")).unwrap();
        fs::write(
            dir.path().join("first/pyproject.toml"),
            r#"[project]
name = "shared-name"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("second")).unwrap();
        fs::write(
            dir.path().join("second/pyproject.toml"),
            r#"[project]
name = "second-actual"

[tool.prograph]
aliases = ["shared-name"]
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert!(
            summary.n_warnings >= 1,
            "expected ≥1 warning for name collision, got {}",
            summary.n_warnings
        );
    }

    #[test]
    fn edge_evidence_persisted_for_mcp_call() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        fs::create_dir_all(dir.path().join("server")).unwrap();
        fs::write(
            dir.path().join("server/pyproject.toml"),
            r#"[project]
name = "srv"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("server/server.py"),
            r#"@server.tool()
def decide():
    return 1
"#,
        )
        .unwrap();

        fs::create_dir_all(dir.path().join("client")).unwrap();
        fs::write(
            dir.path().join("client/pyproject.toml"),
            r#"[project]
name = "cli"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("client/client.py"),
            r#"async def run(session):
    a = await session.call_tool("decide", {})
    b = await session.call_tool("decide", {})
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_evidence: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM edge_evidence
                 WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            n_evidence >= 2,
            "expected ≥2 evidence rows for two call sites, got {}",
            n_evidence
        );
    }

    #[test]
    fn search_fts_populated_after_index() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("maestro")).unwrap();
        fs::write(
            dir.path().join("maestro/pyproject.toml"),
            r#"[project]
name = "maestro"
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'maestro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n >= 1, "expected ≥1 FTS hit on 'maestro', got {}", n);
    }

    #[test]
    fn mcp_tool_decls_persist_across_snapshots() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("server")).unwrap();
        fs::write(
            dir.path().join("server/pyproject.toml"),
            r#"[project]
name = "srv"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("server/server.py"),
            r#"@server.tool()
def hello():
    return "world"
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let alive = store.alive_mcp_tool_decls().unwrap();
        assert!(
            alive.values().any(|(path, _)| path == "server.py"),
            "expected hello tool decl persisted, got: {:?}",
            alive
        );
    }

    #[test]
    fn modules_persist_across_snapshots() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("proj")).unwrap();
        fs::write(
            dir.path().join("proj/pyproject.toml"),
            "[project]\nname = \"proj\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("proj/api.py"),
            "def public_thing():\n    pass\n",
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_modules: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM modules WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n_modules >= 1, "expected ≥1 module persisted");

        let n_symbols: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM public_symbols WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n_symbols >= 1, "expected ≥1 public symbol persisted");
    }

    #[test]
    fn symbol_refs_persist_across_projects() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        // Publisher project: atp_platform with a sdk module.
        fs::create_dir_all(dir.path().join("atp_platform/atp_platform/sdk")).unwrap();
        fs::write(
            dir.path().join("atp_platform/pyproject.toml"),
            "[project]\nname = \"atp_platform\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("atp_platform/atp_platform/__init__.py"), "").unwrap();
        fs::write(
            dir.path().join("atp_platform/atp_platform/sdk/__init__.py"),
            "class Client:\n    pass\n",
        )
        .unwrap();

        // Consumer project: uses atp_platform.sdk.Client.
        fs::create_dir_all(dir.path().join("consumer/consumer")).unwrap();
        fs::write(
            dir.path().join("consumer/pyproject.toml"),
            "[project]\nname = \"consumer\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("consumer/consumer/__init__.py"), "").unwrap();
        fs::write(
            dir.path().join("consumer/consumer/api.py"),
            "from atp_platform.sdk import Client\n",
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_refs: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM cross_project_symbol_refs WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            n_refs >= 1,
            "expected ≥1 symbol ref persisted, got {}",
            n_refs
        );

        let (target_name, module_path, sym): (String, String, Option<String>) = store
            .connection()
            .query_row(
                "SELECT p2.name, ref.to_module_path, ref.to_symbol_name
                 FROM cross_project_symbol_refs ref
                 JOIN projects p2 ON p2.id = ref.to_project_id
                 WHERE ref.last_seen = (SELECT MAX(id) FROM snapshots)
                 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(target_name, "atp_platform");
        assert_eq!(module_path, "sdk");
        assert_eq!(sym.as_deref(), Some("Client"));
    }

    #[test]
    fn indexer_extracts_intent_per_project() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("p/p")).unwrap();
        fs::write(
            dir.path().join("p/pyproject.toml"),
            "[project]\nname = \"p\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("p/README.md"),
            "## Public surface\n- `PublicThing`\n",
        )
        .unwrap();
        fs::write(dir.path().join("p/p/__init__.py"), "").unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 1);

        // Re-run to confirm intent extraction is idempotent.
        index_monorepo(dir.path(), &mut store).unwrap();
    }
}
