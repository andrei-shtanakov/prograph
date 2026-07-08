//! SQLite-backed graph store. M1 only opens the DB and applies the v1 schema.

use std::path::Path;

use rusqlite::Connection;

use crate::errors::{PrographError, Result};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/v1.sql")),
    (2, include_str!("migrations/v2.sql")),
    (3, include_str!("migrations/v3.sql")),
    (4, include_str!("migrations/v4.sql")),
    (5, include_str!("migrations/v5.sql")),
    (6, include_str!("migrations/v6.sql")),
    (7, include_str!("migrations/v7.sql")),
    (8, include_str!("migrations/v8.sql")),
    (9, include_str!("migrations/v9.sql")),
];

/// Sanitize an identifier (project name or contract declared_id) into a filesystem-safe
/// filename slug. Replaces any character that isn't ASCII alphanumeric, dash, or underscore
/// with `-`. Preserves case. Empty input → "_unnamed".
fn slugify(s: &str) -> String {
    if s.is_empty() {
        return "_unnamed".to_string();
    }
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Slug for a contract — declared_id if present and non-empty, else first 12 chars of content_hash.
fn contract_slug(declared_id: Option<&str>, content_hash: &str) -> String {
    match declared_id {
        Some(id) if !id.is_empty() => slugify(id),
        _ => format!("hash-{}", &content_hash[..content_hash.len().min(12)]),
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the SQLite DB at `path` and apply any pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PrographError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let conn = Connection::open(path)?;

        let current: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version
                 WHERE EXISTS (SELECT 1 FROM sqlite_master
                               WHERE type='table' AND name='schema_version')",
                [],
                |r| r.get(0),
            )
            .or_else(|_| {
                // schema_version table does not exist yet — first run, version is effectively 0.
                Ok::<i64, rusqlite::Error>(0)
            })?;

        // Migrations may use `ALTER TABLE ... RENAME` for CHECK widening (v3). SQLite ≥3.26
        // auto-updates FK references in other tables to the new name by default — which here
        // means edge_evidence's FK gets pointed at the temporary `_edges_v2` and then dangles
        // when we DROP it. `legacy_alter_table = ON` keeps the rename literal, so the FK
        // continues to reference `edges` (the recreated table). Disable FK enforcement during
        // the migration window in any case.
        conn.execute("PRAGMA foreign_keys = OFF;", [])?;
        conn.execute("PRAGMA legacy_alter_table = ON;", [])?;
        for (version, sql) in MIGRATIONS {
            if *version > current {
                conn.execute_batch(sql)?;
            }
        }
        conn.execute("PRAGMA legacy_alter_table = OFF;", [])?;
        conn.execute("PRAGMA foreign_keys = ON;", [])?;

        Ok(Self { conn })
    }

    /// Return the highest applied schema version.
    pub fn schema_version(&self) -> Result<i64> {
        let v: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    /// Return the alive set of projects: (root_path -> (project_id, attrs_json)).
    /// "Alive" means `last_seen == MAX(snapshots.id)`.
    pub fn alive_projects(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, root_path, attrs_json FROM projects
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, root, attrs) = row?;
            out.insert(root, (id, attrs));
        }
        Ok(out)
    }

    /// Return the alive set of edges keyed by identity tuple, value = (edge_id, attrs_json).
    /// Identity key: "{kind}|{from_kind}|{from_id}|{to_kind}|{to_id}|{attrs_hash}".
    pub fn alive_edges(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, from_kind, from_id, to_kind, to_id, attrs_hash, attrs_json
             FROM edges
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        for row in rows {
            let (id, kind, fk, fi, tk, ti, ah, aj) = row?;
            let key = format!("{kind}|{fk}|{fi}|{tk}|{ti}|{ah}");
            out.insert(key, (id, aj));
        }
        Ok(out)
    }

    /// Begin a transaction, returning a guard. Use methods on the returned `SnapshotWriter`
    /// to populate the snapshot. Commits on `.commit()`, rolls back on drop without commit.
    pub fn begin_snapshot(&mut self) -> Result<SnapshotWriter<'_>> {
        let tx = self.conn.transaction()?;
        Ok(SnapshotWriter { tx })
    }

    /// Return alive MCP tool decls keyed by "{project_id}|{tool_name}" -> (rel_path, line).
    pub fn alive_mcp_tool_decls(&self) -> Result<std::collections::HashMap<String, (String, i64)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT project_id, tool_name, rel_path, line FROM mcp_tool_decls
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (pid, name, path, line) = row?;
            let key = format!("{}|{}", pid, name);
            out.insert(key, (path, line));
        }
        Ok(out)
    }

    /// Return the alive set of contracts keyed by identity:
    /// "{declared_id_or_empty}|{content_hash}" -> (contract_id, kind_str).
    pub fn alive_contracts(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, declared_id, content_hash, kind FROM contracts
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, declared_id, content_hash, kind) = row?;
            let key = format!("{}|{}", declared_id.as_deref().unwrap_or(""), content_hash);
            out.insert(key, (id, kind));
        }
        Ok(out)
    }

    /// Build a complete `ProjectDescription` for one project at the latest snapshot.
    /// Returns `None` if the project doesn't exist in the latest snapshot.
    pub fn describe_project(
        &self,
        project_id: i64,
    ) -> Result<Option<crate::models::ProjectDescription>> {
        use crate::models::*;

        let snap_meta = self.conn.query_row(
            "SELECT id, ts FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        );
        let (snap_id, snap_ts) = match snap_meta {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let proj = self.conn.query_row(
            "SELECT id, name, kind, root_path, attrs_json FROM projects
             WHERE id = ? AND last_seen = ?",
            rusqlite::params![project_id, snap_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        );
        let (pid, name, kind, root_path, attrs_json) = match proj {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let mut decls: Vec<McpToolDeclRow> = self
            .conn
            .prepare(
                "SELECT tool_name, rel_path, line FROM mcp_tool_decls
                 WHERE project_id = ? AND last_seen = ?
                 ORDER BY tool_name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(McpToolDeclRow {
                    tool_name: r.get(0)?,
                    rel_path: r.get(1)?,
                    line: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        decls.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));

        let contract_files: Vec<ContractFileRow> = self
            .conn
            .prepare(
                "SELECT c.declared_id, c.content_hash, c.kind, cf.rel_path
                 FROM contract_files cf
                 JOIN contracts c ON c.id = cf.contract_id
                 WHERE cf.project_id = ? AND cf.last_seen = ?
                 ORDER BY COALESCE(c.declared_id, c.content_hash), cf.rel_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                let declared_id: Option<String> = r.get(0)?;
                let content_hash: String = r.get(1)?;
                Ok(ContractFileRow {
                    contract_slug: contract_slug(declared_id.as_deref(), &content_hash),
                    contract_declared_id: declared_id,
                    contract_kind: r.get(2)?,
                    rel_path: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        // Outbound edges: gather raw rows first, then resolve target slugs.
        let outbound_raw: Vec<(String, String, String, i64, String)> = self
            .conn
            .prepare(
                "SELECT e.kind, e.to_kind, e.attrs_json, e.to_id,
                        CASE e.to_kind
                            WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                            WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                        END AS target_name
                 FROM edges e
                 WHERE e.from_kind = 'project' AND e.from_id = ? AND e.last_seen = ?
                 ORDER BY e.kind, target_name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut outbound: Vec<OutboundEdge> = Vec::with_capacity(outbound_raw.len());
        for (kind, target_kind, attrs_json, to_id, target_name) in outbound_raw {
            let target_slug = if target_kind == "contract" {
                let row: rusqlite::Result<(Option<String>, String)> = self.conn.query_row(
                    "SELECT declared_id, content_hash FROM contracts WHERE id = ?",
                    rusqlite::params![to_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                );
                match row {
                    Ok((d, h)) => contract_slug(d.as_deref(), &h),
                    Err(_) => "unknown".into(),
                }
            } else {
                slugify(&target_name)
            };
            outbound.push(OutboundEdge {
                kind,
                target_kind,
                target_name,
                target_slug,
                attrs_json,
            });
        }

        let inbound: Vec<InboundEdge> = self
            .conn
            .prepare(
                "SELECT e.kind, e.attrs_json, p.name
                 FROM edges e
                 JOIN projects p ON p.id = e.from_id
                 WHERE e.to_kind = 'project' AND e.to_id = ? AND e.last_seen = ?
                 ORDER BY e.kind, p.name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                let source_name: String = r.get(2)?;
                Ok(InboundEdge {
                    kind: r.get(0)?,
                    attrs_json: r.get(1)?,
                    source_slug: slugify(&source_name),
                    source_name,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, change
                 FROM change_log
                 WHERE entity_kind = 'project' AND entity_id = ?
                 ORDER BY snapshot_id DESC LIMIT 5",
            )?
            .query_map(rusqlite::params![pid], |r| {
                let change: String = r.get(2)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("project {}", change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let modules: Vec<ModuleRow> = self
            .conn
            .prepare(
                "SELECT id, rel_path, language FROM modules
                 WHERE project_id = ? AND last_seen = ?
                 ORDER BY rel_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(ModuleRow {
                    id: r.get(0)?,
                    rel_path: r.get(1)?,
                    language: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let public_symbols: Vec<PublicSymbolRow> = self
            .conn
            .prepare(
                "SELECT ps.module_id, m.rel_path, ps.name, ps.kind, ps.line
                 FROM public_symbols ps
                 JOIN modules m ON m.id = ps.module_id
                 WHERE m.project_id = ? AND ps.last_seen = ?
                 ORDER BY m.rel_path, ps.line, ps.name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(PublicSymbolRow {
                    module_id: r.get(0)?,
                    rel_path: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    line: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let internal_imports: Vec<InternalImportRow> = self
            .conn
            .prepare(
                "SELECT ii.module_id, m.rel_path, ii.target_path, ii.line
                 FROM internal_imports ii
                 JOIN modules m ON m.id = ii.module_id
                 WHERE m.project_id = ? AND ii.last_seen = ?
                 ORDER BY m.rel_path, ii.line, ii.target_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(InternalImportRow {
                    module_id: r.get(0)?,
                    rel_path: r.get(1)?,
                    target_path: r.get(2)?,
                    line: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        // M10: cross-project symbol refs for both directions.
        let inbound_refs = self.refs_to_symbol(&name, None)?;
        let outbound_refs = self.refs_from_project(&name)?;

        let drifts = self.drifts_for_project(&name)?;

        Ok(Some(ProjectDescription {
            project_id: pid,
            slug: slugify(&name),
            name,
            kind,
            root_path,
            attrs_json,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            mcp_decls: decls,
            contract_files,
            outbound,
            inbound,
            recent_changes,
            modules,
            public_symbols,
            internal_imports,
            inbound_refs,
            outbound_refs,
            drifts,
        }))
    }

    pub fn describe_contract(
        &self,
        contract_id: i64,
    ) -> Result<Option<crate::models::ContractDescription>> {
        use crate::models::*;

        let snap_meta = self.conn.query_row(
            "SELECT id, ts FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        );
        let (snap_id, snap_ts) = match snap_meta {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let cont = self.conn.query_row(
            "SELECT id, declared_id, content_hash, kind FROM contracts
             WHERE id = ? AND last_seen = ?",
            rusqlite::params![contract_id, snap_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        );
        let (cid, declared_id, content_hash, kind) = match cont {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let owners: Vec<ContractOwner> = self
            .conn
            .prepare(
                "SELECT p.name, cf.rel_path
                 FROM contract_files cf
                 JOIN projects p ON p.id = cf.project_id
                 WHERE cf.contract_id = ? AND cf.last_seen = ?
                 ORDER BY p.name, cf.rel_path",
            )?
            .query_map(rusqlite::params![cid, snap_id], |r| {
                let project_name: String = r.get(0)?;
                Ok(ContractOwner {
                    project_slug: slugify(&project_name),
                    project_name,
                    rel_path: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, change
                 FROM change_log
                 WHERE entity_kind = 'contract' AND entity_id = ?
                 ORDER BY snapshot_id DESC LIMIT 5",
            )?
            .query_map(rusqlite::params![cid], |r| {
                let change: String = r.get(2)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("contract {}", change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Some(ContractDescription {
            contract_id: cid,
            slug: contract_slug(declared_id.as_deref(), &content_hash),
            declared_id,
            content_hash,
            kind,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            owners,
            recent_changes,
        }))
    }

    pub fn monorepo_overview(&self) -> Result<Option<crate::models::MonorepoOverview>> {
        use crate::models::*;

        let snap = self.conn.query_row(
            "SELECT id, ts, monorepo_root FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        );
        let (snap_id, snap_ts, monorepo_root) = match snap {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let projects: Vec<ProjectSummary> = self
            .conn
            .prepare(
                "SELECT name, kind FROM projects
                 WHERE last_seen = ?
                 ORDER BY name",
            )?
            .query_map(rusqlite::params![snap_id], |r| {
                let name: String = r.get(0)?;
                Ok(ProjectSummary {
                    slug: slugify(&name),
                    name,
                    kind: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let contracts: Vec<ContractSummary> = self
            .conn
            .prepare(
                "SELECT c.declared_id, c.content_hash, c.kind,
                        (SELECT COUNT(DISTINCT cf.project_id)
                         FROM contract_files cf
                         WHERE cf.contract_id = c.id AND cf.last_seen = ?) AS n_owners
                 FROM contracts c
                 WHERE c.last_seen = ?
                 ORDER BY COALESCE(c.declared_id, c.content_hash)",
            )?
            .query_map(rusqlite::params![snap_id, snap_id], |r| {
                let declared_id: Option<String> = r.get(0)?;
                let content_hash: String = r.get(1)?;
                Ok(ContractSummary {
                    slug: contract_slug(declared_id.as_deref(), &content_hash),
                    declared_id,
                    kind: r.get(2)?,
                    n_owners: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let n_edges: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE last_seen = ?",
            rusqlite::params![snap_id],
            |r| r.get(0),
        )?;

        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, entity_kind, change
                 FROM change_log
                 ORDER BY snapshot_id DESC LIMIT 10",
            )?
            .query_map([], |r| {
                let entity_kind: String = r.get(2)?;
                let change: String = r.get(3)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("{} {}", entity_kind, change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Some(MonorepoOverview {
            monorepo_root,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            n_projects: projects.len() as i64,
            n_contracts: contracts.len() as i64,
            n_edges,
            projects,
            contracts,
            recent_changes,
        }))
    }

    /// Resolve a project name → id at the latest snapshot. None if not found.
    pub fn project_by_name(&self, name: &str) -> Result<Option<i64>> {
        let row = self.conn.query_row(
            "SELECT id FROM projects
             WHERE name = ? AND last_seen = (SELECT MAX(id) FROM snapshots)",
            rusqlite::params![name],
            |r| r.get::<_, i64>(0),
        );
        match row {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// SnapshotInfo for an arbitrary snapshot id.
    pub fn snapshot_by_id(&self, id: i64) -> Result<Option<crate::models::SnapshotInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, monorepo_root, git_commit, prograph_version,
                    (SELECT COUNT(*) FROM projects WHERE last_seen = s.id) AS n_projects,
                    (SELECT COUNT(*) FROM edges    WHERE last_seen = s.id) AS n_edges,
                    (SELECT COUNT(*) FROM change_log WHERE snapshot_id = s.id) AS n_changes
             FROM snapshots s
             WHERE s.id = ?",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(crate::models::SnapshotInfo {
                id: r.get(0)?,
                ts: r.get(1)?,
                monorepo_root: r.get(2)?,
                git_commit: r.get(3)?,
                prograph_version: r.get(4)?,
                n_projects: r.get(5)?,
                n_edges: r.get(6)?,
                n_changes: r.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Query edges with optional filters. All four predicates are AND'ed.
    pub fn find_edges_filtered(
        &self,
        from_name: Option<&str>,
        to_name: Option<&str>,
        kind: Option<&str>,
        since_snapshot: Option<i64>,
    ) -> Result<Vec<crate::models::EdgeRow>> {
        let mut sql = String::from(
            "SELECT e.id, e.kind, e.from_kind, e.from_id,
                    CASE e.from_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.from_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.from_id)
                    END AS from_name,
                    e.to_kind, e.to_id,
                    CASE e.to_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                    END AS to_name,
                    e.attrs_json, e.first_seen, e.last_seen
             FROM edges e
             WHERE e.last_seen = (SELECT MAX(id) FROM snapshots)",
        );
        if kind.is_some() {
            sql.push_str(" AND e.kind = ?");
        }
        if from_name.is_some() {
            sql.push_str(" AND from_name = ?");
        }
        if to_name.is_some() {
            sql.push_str(" AND to_name = ?");
        }
        if since_snapshot.is_some() {
            sql.push_str(" AND e.first_seen >= ?");
        }
        sql.push_str(" ORDER BY e.kind, from_name, to_name");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(k) = kind {
            params.push(Box::new(k.to_string()));
        }
        if let Some(f) = from_name {
            params.push(Box::new(f.to_string()));
        }
        if let Some(t) = to_name {
            params.push(Box::new(t.to_string()));
        }
        if let Some(s) = since_snapshot {
            params.push(Box::new(s));
        }

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::EdgeRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                from_kind: r.get(2)?,
                from_id: r.get(3)?,
                from_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                to_kind: r.get(5)?,
                to_id: r.get(6)?,
                to_name: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                attrs_json: r.get(8)?,
                first_seen: r.get(9)?,
                last_seen: r.get(10)?,
            })
        })?;

        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Return ALL edges visible in the diff between `since_snapshot` and the current
    /// latest snapshot, each tagged with status `added` / `removed` / `unchanged`.
    ///
    /// Identity rules (mirroring spec §5.2):
    /// - `added`:     first_seen > since AND last_seen = max_snap
    /// - `removed`:   last_seen >= since AND last_seen < max_snap
    /// - `unchanged`: first_seen <= since AND last_seen = max_snap
    pub fn find_edges_with_status_since(
        &self,
        since_snapshot: i64,
    ) -> Result<Vec<crate::models::DiffEdgeRow>> {
        let max_snap: i64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(id), 0) FROM snapshots", [], |r| {
                    r.get(0)
                })?;

        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.kind, e.from_kind, e.from_id,
                    CASE e.from_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.from_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.from_id)
                    END AS from_name,
                    e.to_kind, e.to_id,
                    CASE e.to_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                    END AS to_name,
                    e.attrs_json, e.first_seen, e.last_seen,
                    CASE
                        WHEN e.last_seen = ?1 AND e.first_seen > ?2 THEN 'added'
                        WHEN e.last_seen >= ?2 AND e.last_seen < ?1 THEN 'removed'
                        WHEN e.last_seen = ?1                       THEN 'unchanged'
                        ELSE NULL
                    END AS status
             FROM edges e
             WHERE (e.last_seen = ?1 AND e.first_seen > ?2)
                OR (e.last_seen >= ?2 AND e.last_seen < ?1)
                OR (e.last_seen = ?1 AND e.first_seen <= ?2)
             ORDER BY e.kind, from_name, to_name",
        )?;

        let rows = stmt.query_map(rusqlite::params![max_snap, since_snapshot], |r| {
            Ok(crate::models::DiffEdgeRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                from_kind: r.get(2)?,
                from_id: r.get(3)?,
                from_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                to_kind: r.get(5)?,
                to_id: r.get(6)?,
                to_name: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                attrs_json: r.get(8)?,
                first_seen: r.get(9)?,
                last_seen: r.get(10)?,
                status: r
                    .get::<_, Option<String>>(11)?
                    .unwrap_or_else(|| "unknown".into()),
            })
        })?;

        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// All evidence rows for one edge at the latest snapshot.
    pub fn edge_evidence_for(&self, edge_id: i64) -> Result<Vec<crate::models::EdgeEvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT ev.edge_id, ev.project_id, p.name, ev.rel_path, ev.line, ev.snippet
             FROM edge_evidence ev
             JOIN projects p ON p.id = ev.project_id
             WHERE ev.edge_id = ? AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
             ORDER BY ev.rel_path, ev.line",
        )?;
        let rows = stmt.query_map(rusqlite::params![edge_id], |r| {
            Ok(crate::models::EdgeEvidenceRow {
                edge_id: r.get(0)?,
                project_id: r.get(1)?,
                project_name: r.get(2)?,
                rel_path: r.get(3)?,
                line: r.get(4)?,
                snippet: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// FTS search over project + contract names/bodies in the latest snapshot.
    pub fn search_fts(
        &self,
        query: &str,
        kinds: Option<Vec<String>>,
        limit: i64,
    ) -> Result<Vec<crate::models::SearchHit>> {
        let mut sql = String::from(
            "SELECT entity_kind, entity_id, name,
                    snippet(search_fts, 4, '[', ']', '…', 16) AS hit,
                    bm25(search_fts) AS rank
             FROM search_fts
             WHERE search_fts MATCH ? AND snapshot_id = (SELECT MAX(id) FROM snapshots)",
        );
        if let Some(ref ks) = kinds {
            sql.push_str(" AND entity_kind IN (");
            sql.push_str(&vec!["?"; ks.len()].join(","));
            sql.push(')');
        }
        sql.push_str(" ORDER BY rank LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        if let Some(ks) = kinds {
            for k in ks {
                params.push(Box::new(k));
            }
        }
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::SearchHit {
                entity_kind: r.get(0)?,
                entity_id: r.get(1)?,
                name: r.get(2)?,
                snippet: r.get(3)?,
                rank: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// M10: refs pointing AT a project (optionally filtered by symbol name).
    /// Answers "who imports my X?"
    pub fn refs_to_symbol(
        &self,
        project_name: &str,
        symbol_name: Option<&str>,
    ) -> Result<Vec<crate::models::SymbolRefRow>> {
        let mut sql = String::from(
            "SELECT p1.name, m.rel_path, ref.line, p2.name, ref.to_module_path, ref.to_symbol_name
             FROM cross_project_symbol_refs ref
             JOIN projects p1 ON p1.id = ref.from_project_id
             JOIN modules m ON m.id = ref.from_module_id
             JOIN projects p2 ON p2.id = ref.to_project_id
             WHERE p2.name = ? AND ref.last_seen = (SELECT MAX(id) FROM snapshots)",
        );
        if symbol_name.is_some() {
            sql.push_str(" AND ref.to_symbol_name = ?");
        }
        sql.push_str(" ORDER BY p1.name, m.rel_path, ref.line");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_name.to_string())];
        if let Some(s) = symbol_name {
            params.push(Box::new(s.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::SymbolRefRow {
                from_project_name: r.get(0)?,
                from_module_rel_path: r.get(1)?,
                line: r.get(2)?,
                to_project_name: r.get(3)?,
                to_module_path: r.get(4)?,
                to_symbol_name: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// M10: refs originating FROM a project. Answers "who do I import from?"
    pub fn refs_from_project(
        &self,
        project_name: &str,
    ) -> Result<Vec<crate::models::SymbolRefRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT p1.name, m.rel_path, ref.line, p2.name, ref.to_module_path, ref.to_symbol_name
             FROM cross_project_symbol_refs ref
             JOIN projects p1 ON p1.id = ref.from_project_id
             JOIN modules m ON m.id = ref.from_module_id
             JOIN projects p2 ON p2.id = ref.to_project_id
             WHERE p1.name = ? AND ref.last_seen = (SELECT MAX(id) FROM snapshots)
             ORDER BY p2.name, ref.to_module_path, m.rel_path, ref.line",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_name], |r| {
            Ok(crate::models::SymbolRefRow {
                from_project_name: r.get(0)?,
                from_module_rel_path: r.get(1)?,
                line: r.get(2)?,
                to_project_name: r.get(3)?,
                to_module_path: r.get(4)?,
                to_symbol_name: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Paginated change-log query. since_snapshot = inclusive lower bound on snapshot_id.
    pub fn changelog_paginated(
        &self,
        since_snapshot: Option<i64>,
        entity_kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::models::ChangeEvent>> {
        let mut sql = String::from(
            "SELECT id, snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json
             FROM change_log WHERE 1=1",
        );
        if since_snapshot.is_some() {
            sql.push_str(" AND snapshot_id >= ?");
        }
        if entity_kind.is_some() {
            sql.push_str(" AND entity_kind = ?");
        }
        sql.push_str(" ORDER BY snapshot_id DESC, id DESC LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = since_snapshot {
            params.push(Box::new(s));
        }
        if let Some(k) = entity_kind {
            params.push(Box::new(k.to_string()));
        }
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            let entity_kind_str: String = r.get(3)?;
            let change_str: String = r.get(5)?;
            Ok(crate::models::ChangeEvent {
                id: r.get(0)?,
                snapshot_id: r.get(1)?,
                ts: r.get(2)?,
                entity_kind: match entity_kind_str.as_str() {
                    "project" => crate::models::EntityKind::Project,
                    "edge" => crate::models::EntityKind::Edge,
                    "contract" => crate::models::EntityKind::Contract,
                    _ => crate::models::EntityKind::Edge,
                },
                entity_id: r.get(4)?,
                change: match change_str.as_str() {
                    "added" => crate::models::ChangeKind::Added,
                    "removed" => crate::models::ChangeKind::Removed,
                    "attrs_changed" => crate::models::ChangeKind::AttrsChanged,
                    _ => crate::models::ChangeKind::Added,
                },
                before_json: r.get(6)?,
                after_json: r.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Latest SnapshotInfo if any snapshot exists; None otherwise.
    pub fn latest_snapshot_info(&self) -> Result<Option<crate::models::SnapshotInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, monorepo_root, git_commit, prograph_version,
                    (SELECT COUNT(*) FROM projects WHERE last_seen = s.id) AS n_projects,
                    (SELECT COUNT(*) FROM edges    WHERE last_seen = s.id) AS n_edges,
                    (SELECT COUNT(*) FROM change_log WHERE snapshot_id = s.id) AS n_changes
             FROM snapshots s
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(r) = rows.next()? {
            Ok(Some(crate::models::SnapshotInfo {
                id: r.get(0)?,
                ts: r.get(1)?,
                monorepo_root: r.get(2)?,
                git_commit: r.get(3)?,
                prograph_version: r.get(4)?,
                n_projects: r.get(5)?,
                n_edges: r.get(6)?,
                n_changes: r.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// M11: synthesize label strings from recent change_log rows. Used by the
    /// drift detector's stale-TODO heuristic. Returns labels of the form
    /// "<entity_kind> <entity_id> <change> <after_json>" — e.g.
    /// "edge 42 added {...}".
    pub fn recent_changelog_labels(&self, n_snapshots: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT entity_kind, entity_id, change, COALESCE(after_json, '')
             FROM change_log
             WHERE snapshot_id >= (SELECT COALESCE(MAX(id) - ? + 1, 0) FROM snapshots)
             ORDER BY snapshot_id DESC, id DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![n_snapshots], |r| {
            let entity_kind: String = r.get(0)?;
            let entity_id: i64 = r.get(1)?;
            let change: String = r.get(2)?;
            let after: String = r.get(3)?;
            Ok(format!(
                "{} {} {} {}",
                entity_kind, entity_id, change, after
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// M11: all drift findings for one project at the current snapshot.
    pub fn drifts_for_project(
        &self,
        project_name: &str,
    ) -> Result<Vec<crate::models::DriftFindingRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.name, df.kind, df.entity_kind, df.entity_name,
                    df.source_path, df.source_line, df.confidence, df.detail
             FROM drift_findings df
             JOIN projects p ON p.id = df.project_id
             WHERE p.name = ? AND df.last_seen = (SELECT MAX(id) FROM snapshots)
             ORDER BY df.kind, df.entity_kind, df.entity_name",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_name], |r| {
            Ok(crate::models::DriftFindingRow {
                project_name: r.get(0)?,
                kind: r.get(1)?,
                entity_kind: r.get(2)?,
                entity_name: r.get(3)?,
                source_path: r.get(4)?,
                source_line: r.get(5)?,
                confidence: r.get(6)?,
                detail: r.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// M11: all drift findings across projects, optionally filtered by kind.
    pub fn find_drifts_filtered(
        &self,
        kind: Option<&str>,
    ) -> Result<Vec<crate::models::DriftFindingRow>> {
        if let Some(k) = kind {
            let mut stmt = self.conn.prepare(
                "SELECT p.name, df.kind, df.entity_kind, df.entity_name,
                        df.source_path, df.source_line, df.confidence, df.detail
                 FROM drift_findings df
                 JOIN projects p ON p.id = df.project_id
                 WHERE df.kind = ? AND df.last_seen = (SELECT MAX(id) FROM snapshots)
                 ORDER BY p.name, df.entity_kind, df.entity_name",
            )?;
            let rows = stmt.query_map(rusqlite::params![k], |r| {
                Ok(crate::models::DriftFindingRow {
                    project_name: r.get(0)?,
                    kind: r.get(1)?,
                    entity_kind: r.get(2)?,
                    entity_name: r.get(3)?,
                    source_path: r.get(4)?,
                    source_line: r.get(5)?,
                    confidence: r.get(6)?,
                    detail: r.get(7)?,
                })
            })?;
            rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT p.name, df.kind, df.entity_kind, df.entity_name,
                        df.source_path, df.source_line, df.confidence, df.detail
                 FROM drift_findings df
                 JOIN projects p ON p.id = df.project_id
                 WHERE df.last_seen = (SELECT MAX(id) FROM snapshots)
                 ORDER BY p.name, df.kind, df.entity_kind, df.entity_name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(crate::models::DriftFindingRow {
                    project_name: r.get(0)?,
                    kind: r.get(1)?,
                    entity_kind: r.get(2)?,
                    entity_name: r.get(3)?,
                    source_path: r.get(4)?,
                    source_line: r.get(5)?,
                    confidence: r.get(6)?,
                    detail: r.get(7)?,
                })
            })?;
            rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
        }
    }
}

/// Transactional writer for a single snapshot.
///
/// Drop without commit = ROLLBACK. Methods on the writer accumulate operations
/// inside the transaction; nothing is visible to other readers until `commit()`.
pub struct SnapshotWriter<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl SnapshotWriter<'_> {
    /// Insert the new snapshots row and return its id.
    pub fn insert_snapshot(
        &self,
        ts: &str,
        monorepo_root: &str,
        git_commit: Option<&str>,
        prograph_version: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO snapshots (ts, monorepo_root, git_commit, prograph_version)
             VALUES (?, ?, ?, ?)",
            rusqlite::params![ts, monorepo_root, git_commit, prograph_version],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    /// Insert a new project row; returns its id.
    pub fn insert_project(
        &self,
        snapshot_id: i64,
        name: &str,
        root_path: &str,
        kind: &str,
        attrs_json: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![name, root_path, kind, attrs_json, snapshot_id, snapshot_id],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    /// Extend an existing project's last_seen to the current snapshot, optionally updating attrs_json.
    pub fn touch_project(
        &self,
        project_id: i64,
        snapshot_id: i64,
        new_attrs_json: Option<&str>,
    ) -> Result<()> {
        if let Some(attrs) = new_attrs_json {
            self.tx.execute(
                "UPDATE projects SET last_seen = ?, attrs_json = ? WHERE id = ?",
                rusqlite::params![snapshot_id, attrs, project_id],
            )?;
        } else {
            self.tx.execute(
                "UPDATE projects SET last_seen = ? WHERE id = ?",
                rusqlite::params![snapshot_id, project_id],
            )?;
        }
        Ok(())
    }

    // Wide signature mirrors the edges schema; refactoring into a struct would
    // just push the same fields one level deeper without clarity gains.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_edge(
        &self,
        snapshot_id: i64,
        kind: &str,
        from_kind: &str,
        from_id: i64,
        to_kind: &str,
        to_id: i64,
        attrs_json: &str,
        attrs_hash: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO edges (kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash,
                snapshot_id, snapshot_id
            ],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    pub fn touch_edge(
        &self,
        edge_id: i64,
        snapshot_id: i64,
        new_attrs_json: Option<&str>,
    ) -> Result<()> {
        if let Some(attrs) = new_attrs_json {
            self.tx.execute(
                "UPDATE edges SET last_seen = ?, attrs_json = ? WHERE id = ?",
                rusqlite::params![snapshot_id, attrs, edge_id],
            )?;
        } else {
            self.tx.execute(
                "UPDATE edges SET last_seen = ? WHERE id = ?",
                rusqlite::params![snapshot_id, edge_id],
            )?;
        }
        Ok(())
    }

    // Wide signature mirrors the change_log schema; same rationale as insert_edge.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_change_log(
        &self,
        snapshot_id: i64,
        ts: &str,
        entity_kind: &str,
        entity_id: i64,
        change: &str,
        before_json: Option<&str>,
        after_json: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT INTO change_log (snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json
            ],
        )?;
        Ok(())
    }

    pub fn insert_contract(
        &self,
        snapshot_id: i64,
        declared_id: Option<&str>,
        content_hash: &str,
        kind: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO contracts (declared_id, content_hash, kind, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![declared_id, content_hash, kind, snapshot_id, snapshot_id],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    pub fn touch_contract(&self, contract_id: i64, snapshot_id: i64) -> Result<()> {
        self.tx.execute(
            "UPDATE contracts SET last_seen = ? WHERE id = ?",
            rusqlite::params![snapshot_id, contract_id],
        )?;
        Ok(())
    }

    pub fn insert_contract_file(
        &self,
        contract_id: i64,
        project_id: i64,
        rel_path: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR IGNORE INTO contract_files
             (contract_id, project_id, rel_path, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![contract_id, project_id, rel_path, snapshot_id, snapshot_id],
        )?;
        Ok(())
    }

    pub fn touch_contract_file(
        &self,
        contract_id: i64,
        project_id: i64,
        rel_path: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "UPDATE contract_files SET last_seen = ?
             WHERE contract_id = ? AND project_id = ? AND rel_path = ?",
            rusqlite::params![snapshot_id, contract_id, project_id, rel_path],
        )?;
        Ok(())
    }

    pub fn insert_mcp_tool_decl(
        &self,
        project_id: i64,
        tool_name: &str,
        rel_path: &str,
        line: i64,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO mcp_tool_decls
             (project_id, tool_name, rel_path, line, first_seen, last_seen)
             VALUES (?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM mcp_tool_decls WHERE project_id=? AND tool_name=?), ?),
                     ?)",
            rusqlite::params![
                project_id, tool_name, rel_path, line,
                project_id, tool_name, snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    pub fn touch_mcp_tool_decl(
        &self,
        project_id: i64,
        tool_name: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "UPDATE mcp_tool_decls SET last_seen = ?
             WHERE project_id = ? AND tool_name = ?",
            rusqlite::params![snapshot_id, project_id, tool_name],
        )?;
        Ok(())
    }

    pub fn insert_edge_evidence(
        &self,
        edge_id: i64,
        project_id: i64,
        rel_path: &str,
        line: i64,
        snippet: Option<&str>,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO edge_evidence
             (edge_id, project_id, rel_path, line, snippet, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM edge_evidence
                               WHERE edge_id=? AND project_id=? AND rel_path=? AND line=?), ?),
                     ?)",
            rusqlite::params![
                edge_id,
                project_id,
                rel_path,
                line,
                snippet,
                edge_id,
                project_id,
                rel_path,
                line,
                snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    /// M9: insert or upsert a module row; returns the module id.
    pub fn insert_module(
        &self,
        snapshot_id: i64,
        project_id: i64,
        rel_path: &str,
        language: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT OR IGNORE INTO modules (project_id, rel_path, language, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![project_id, rel_path, language, snapshot_id, snapshot_id],
        )?;
        let mid: i64 = self.tx.query_row(
            "SELECT id FROM modules WHERE project_id = ? AND rel_path = ?",
            rusqlite::params![project_id, rel_path],
            |r| r.get(0),
        )?;
        self.tx.execute(
            "UPDATE modules SET last_seen = ? WHERE id = ?",
            rusqlite::params![snapshot_id, mid],
        )?;
        Ok(mid)
    }

    pub fn insert_public_symbol(
        &self,
        module_id: i64,
        snapshot_id: i64,
        name: &str,
        kind: &str,
        line: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO public_symbols
             (module_id, name, kind, line, first_seen, last_seen)
             VALUES (?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM public_symbols
                               WHERE module_id = ? AND name = ?), ?),
                     ?)",
            rusqlite::params![
                module_id,
                name,
                kind,
                line,
                module_id,
                name,
                snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    pub fn insert_internal_import(
        &self,
        module_id: i64,
        snapshot_id: i64,
        target_path: &str,
        line: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO internal_imports
             (module_id, target_path, line, first_seen, last_seen)
             VALUES (?, ?, ?,
                     COALESCE((SELECT first_seen FROM internal_imports
                               WHERE module_id = ? AND target_path = ? AND line = ?), ?),
                     ?)",
            rusqlite::params![
                module_id,
                target_path,
                line,
                module_id,
                target_path,
                line,
                snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    /// M10: insert (or refresh) a cross-project symbol reference row.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_symbol_ref(
        &self,
        snapshot_id: i64,
        from_project_id: i64,
        from_module_id: i64,
        line: i64,
        to_project_id: i64,
        to_module_path: &str,
        to_symbol_name: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO cross_project_symbol_refs
             (from_project_id, from_module_id, line, to_project_id, to_module_path, to_symbol_name, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM cross_project_symbol_refs
                               WHERE from_module_id=? AND line=? AND to_project_id=? AND to_module_path=?
                                 AND COALESCE(to_symbol_name, '') = COALESCE(?, '')), ?),
                     ?)",
            rusqlite::params![
                from_project_id,
                from_module_id,
                line,
                to_project_id,
                to_module_path,
                to_symbol_name,
                from_module_id,
                line,
                to_project_id,
                to_module_path,
                to_symbol_name,
                snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    /// Read-only access to the transaction's underlying connection. Used by the
    /// indexer's M10 resolver pass for module-id lookups within the same tx.
    pub(crate) fn conn(&self) -> &rusqlite::Connection {
        &self.tx
    }

    /// Clear the FTS index for the given snapshot id and repopulate from current state.
    /// Called at the end of the persist phase, after all projects/contracts are written.
    pub fn rebuild_search_fts(&self, snapshot_id: i64) -> Result<()> {
        self.tx.execute(
            "DELETE FROM search_fts WHERE snapshot_id = ?",
            rusqlite::params![snapshot_id],
        )?;

        self.tx.execute(
            "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
             SELECT 'project', id, ?, name,
                    COALESCE(name, '') || ' ' || COALESCE(kind, '') || ' ' ||
                    COALESCE(root_path, '') || ' ' || COALESCE(attrs_json, '')
             FROM projects WHERE last_seen = ?",
            rusqlite::params![snapshot_id, snapshot_id],
        )?;

        self.tx.execute(
            "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
             SELECT 'contract', id, ?, COALESCE(declared_id, content_hash),
                    COALESCE(declared_id, '') || ' ' || COALESCE(kind, '') || ' ' ||
                    SUBSTR(COALESCE(content_hash, ''), 1, 16)
             FROM contracts WHERE last_seen = ?",
            rusqlite::params![snapshot_id, snapshot_id],
        )?;

        Ok(())
    }

    /// M11: insert (or refresh) a drift finding row.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_drift_finding(
        &self,
        snapshot_id: i64,
        project_id: i64,
        kind: &str,
        entity_kind: &str,
        entity_name: &str,
        source_path: &str,
        source_line: i64,
        confidence: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO drift_findings
             (project_id, kind, entity_kind, entity_name, source_path, source_line,
              confidence, detail, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM drift_findings
                               WHERE project_id=? AND kind=? AND entity_kind=? AND entity_name=?
                                 AND source_path=? AND source_line=?), ?),
                     ?)",
            rusqlite::params![
                project_id,
                kind,
                entity_kind,
                entity_name,
                source_path,
                source_line,
                confidence,
                detail,
                project_id,
                kind,
                entity_kind,
                entity_name,
                source_path,
                source_line,
                snapshot_id,
                snapshot_id,
            ],
        )?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_db_and_applies_v1_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".prograph/graph.db");

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn open_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("graph.db");

        let _ = Store::open(&path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn schema_creates_snapshots_and_projects_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"snapshots".to_string()));
        assert!(names.contains(&"projects".to_string()));
        assert!(names.contains(&"schema_version".to_string()));
    }

    #[test]
    fn schema_v2_creates_edges_change_log_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"edges".to_string()));
        assert!(names.contains(&"edge_evidence".to_string()));
        assert!(names.contains(&"change_log".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn alive_projects_empty_before_any_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.alive_projects().unwrap().is_empty());
    }

    #[test]
    fn write_snapshot_then_alive_projects_reflects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer
            .insert_snapshot("2026-05-25T00:00:00Z", "/m", None, "0.1.0")
            .unwrap();
        let pid = writer
            .insert_project(snap, "alpha", "./alpha", "python", "{}")
            .unwrap();
        writer
            .insert_change_log(
                snap,
                "2026-05-25T00:00:00Z",
                "project",
                pid,
                "added",
                None,
                Some("{}"),
            )
            .unwrap();
        writer.commit().unwrap();

        let alive = store.alive_projects().unwrap();
        assert_eq!(alive.len(), 1);
        assert!(alive.contains_key("./alpha"));
    }

    #[test]
    fn latest_snapshot_info_returns_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.latest_snapshot_info().unwrap().is_none());

        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer
                .insert_snapshot("2026-05-25T00:00:00Z", "/m", None, "0.1.0")
                .unwrap();
            writer
                .insert_project(snap, "a", "./a", "python", "{}")
                .unwrap();
            writer.commit().unwrap();
        }

        let info = store.latest_snapshot_info().unwrap().unwrap();
        assert_eq!(info.n_projects, 1);
        assert_eq!(info.n_edges, 0);
        assert_eq!(info.n_changes, 0);
    }

    #[test]
    fn rollback_on_drop_without_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        {
            let writer = store.begin_snapshot().unwrap();
            writer.insert_snapshot("ts", "/m", None, "v").unwrap();
            // No commit — drop rolls back.
        }
        assert!(store.latest_snapshot_info().unwrap().is_none());
    }

    #[test]
    fn migration_is_additive_over_existing_v1_db() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("g.db");

        // Simulate an existing v1 DB by manually applying v1.sql via a raw connection.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("migrations/v1.sql"))
                .unwrap();
            let v: i64 = conn
                .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, 1);
        }

        // Now Store::open should apply v2 + v3.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn schema_v3_creates_contracts_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"contracts".to_string()));
        assert!(names.contains(&"contract_files".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn schema_v3_widens_edges_kind_check() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        // Insert a snapshot + a project + mcp_call/contract_link edges — should succeed under v3.
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer
            .insert_project(snap, "a", "./a", "python", "{}")
            .unwrap();
        let pid_b = writer
            .insert_project(snap, "b", "./b", "python", "{}")
            .unwrap();
        writer
            .insert_edge(
                snap, "mcp_call", "project", pid_a, "project", pid_b, "{}", "h",
            )
            .unwrap();
        writer
            .insert_edge(
                snap,
                "contract_link",
                "project",
                pid_a,
                "contract",
                999,
                "{}",
                "h2",
            )
            .unwrap();
        writer.commit().unwrap();
    }

    #[test]
    fn alive_contracts_empty_before_any_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.alive_contracts().unwrap().is_empty());
    }

    #[test]
    fn write_contract_then_alive_reflects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer
            .insert_project(snap, "a", "./a", "python", "{}")
            .unwrap();
        let cid = writer
            .insert_contract(snap, Some("obs-v1"), "deadbeef", "json_schema")
            .unwrap();
        writer
            .insert_contract_file(cid, pid, "schemas/obs.json", snap)
            .unwrap();
        writer.commit().unwrap();

        let alive = store.alive_contracts().unwrap();
        assert_eq!(alive.len(), 1);
        assert!(alive.contains_key("obs-v1|deadbeef"));
    }

    #[test]
    fn touch_contract_extends_last_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let cid_in_snap1 = {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            let cid = writer
                .insert_contract(snap, Some("x"), "hash", "json_schema")
                .unwrap();
            writer.commit().unwrap();
            cid
        };

        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer.touch_contract(cid_in_snap1, snap).unwrap();
            writer.commit().unwrap();
        }

        let last_seen: i64 = store
            .connection()
            .query_row(
                "SELECT last_seen FROM contracts WHERE id = ?",
                rusqlite::params![cid_in_snap1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            last_seen, 2,
            "touch_contract must extend last_seen to snapshot 2"
        );
    }

    #[test]
    fn migration_v2_to_v3_preserves_existing_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("g.db");

        // Bootstrap a v2 DB by hand and insert a row in the v2 edges table.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("migrations/v1.sql"))
                .unwrap();
            conn.execute_batch(include_str!("migrations/v2.sql"))
                .unwrap();
            conn.execute(
                "INSERT INTO snapshots (ts, monorepo_root, prograph_version) VALUES (?, ?, ?)",
                rusqlite::params!["ts", "/m", "0.1.0"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
                 VALUES (?, ?, ?, ?, 1, 1)",
                rusqlite::params!["a", "./a", "python", "{}"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
                 VALUES (?, ?, ?, ?, 1, 1)",
                rusqlite::params!["b", "./b", "python", "{}"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO edges (kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash, first_seen, last_seen)
                 VALUES ('package_dep', 'project', 1, 'project', 2, '{}', 'h', 1, 1)",
                [],
            )
            .unwrap();
        }

        // Open via Store — v3 migration runs.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 9);

        let edge_count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            edge_count, 1,
            "existing package_dep edge must survive v2 → v3 migration"
        );
    }

    #[test]
    fn schema_v4_creates_mcp_tool_decls_table() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"mcp_tool_decls".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn mcp_tool_decl_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer
            .insert_project(snap, "srv", "./srv", "python", "{}")
            .unwrap();
        writer
            .insert_mcp_tool_decl(pid, "decide", "src/server.py", 42, snap)
            .unwrap();
        writer.commit().unwrap();

        let alive = store.alive_mcp_tool_decls().unwrap();
        let key = format!("{}|decide", pid);
        assert!(alive.contains_key(&key));
        let (path, line) = alive[&key].clone();
        assert_eq!(path, "src/server.py");
        assert_eq!(line, 42);
    }

    #[test]
    fn describe_project_returns_none_for_empty_db() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.describe_project(1).unwrap().is_none());
    }

    #[test]
    fn describe_project_aggregates_full_card() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer
            .insert_project(snap, "alpha", "./alpha", "python", "{}")
            .unwrap();
        let pid_b = writer
            .insert_project(snap, "beta", "./beta", "python", "{}")
            .unwrap();
        writer
            .insert_edge(
                snap,
                "package_dep",
                "project",
                pid_a,
                "project",
                pid_b,
                r#"{"dep_name":"beta"}"#,
                "h1",
            )
            .unwrap();
        writer
            .insert_mcp_tool_decl(pid_a, "decide", "src/server.py", 10, snap)
            .unwrap();
        writer.commit().unwrap();

        let desc = store.describe_project(pid_a).unwrap().unwrap();
        assert_eq!(desc.name, "alpha");
        assert_eq!(desc.outbound.len(), 1);
        assert_eq!(desc.outbound[0].target_name, "beta");
        assert_eq!(desc.mcp_decls.len(), 1);
        assert_eq!(desc.mcp_decls[0].tool_name, "decide");
    }

    #[test]
    fn monorepo_overview_reports_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer
            .insert_project(snap, "alpha", "./alpha", "python", "{}")
            .unwrap();
        let pid_b = writer
            .insert_project(snap, "beta", "./beta", "rust", "{}")
            .unwrap();
        writer
            .insert_edge(
                snap,
                "package_dep",
                "project",
                pid_a,
                "project",
                pid_b,
                "{}",
                "h",
            )
            .unwrap();
        writer.commit().unwrap();

        let ov = store.monorepo_overview().unwrap().unwrap();
        assert_eq!(ov.n_projects, 2);
        assert_eq!(ov.n_edges, 1);
        let names: Vec<_> = ov.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn touch_mcp_tool_decl_preserves_first_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        // Snapshot 1: insert.
        let pid;
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            pid = writer
                .insert_project(snap, "srv", "./srv", "python", "{}")
                .unwrap();
            writer
                .insert_mcp_tool_decl(pid, "decide", "src/server.py", 42, snap)
                .unwrap();
            writer.commit().unwrap();
        }

        // Snapshot 2: re-insert with same identity, different line.
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer
                .insert_mcp_tool_decl(pid, "decide", "src/server.py", 99, snap)
                .unwrap();
            writer.commit().unwrap();
        }

        let row: (i64, i64) = store
            .connection()
            .query_row(
                "SELECT first_seen, last_seen FROM mcp_tool_decls
                 WHERE project_id = ? AND tool_name = 'decide'",
                rusqlite::params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, 1, "first_seen must remain snapshot 1");
        assert_eq!(row.1, 2, "last_seen must advance to snapshot 2");
    }

    #[test]
    fn project_by_name_finds_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer
            .insert_project(snap, "Maestro", "./Maestro", "python", "{}")
            .unwrap();
        writer.commit().unwrap();
        assert_eq!(store.project_by_name("Maestro").unwrap(), Some(pid));
        assert_eq!(store.project_by_name("nope").unwrap(), None);
    }

    #[test]
    fn snapshot_by_id_returns_with_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        writer
            .insert_project(snap, "x", "./x", "python", "{}")
            .unwrap();
        writer.commit().unwrap();
        let info = store.snapshot_by_id(snap).unwrap().unwrap();
        assert_eq!(info.n_projects, 1);
        assert!(store.snapshot_by_id(9999).unwrap().is_none());
    }

    #[test]
    fn find_edges_filtered_by_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pa = writer
            .insert_project(snap, "a", "./a", "python", "{}")
            .unwrap();
        let pb = writer
            .insert_project(snap, "b", "./b", "python", "{}")
            .unwrap();
        writer
            .insert_edge(
                snap,
                "package_dep",
                "project",
                pa,
                "project",
                pb,
                "{}",
                "h1",
            )
            .unwrap();
        writer
            .insert_edge(
                snap,
                "mcp_call",
                "project",
                pa,
                "project",
                pb,
                r#"{"tool":"t"}"#,
                "h2",
            )
            .unwrap();
        writer.commit().unwrap();

        let only_mcp = store
            .find_edges_filtered(None, None, Some("mcp_call"), None)
            .unwrap();
        assert_eq!(only_mcp.len(), 1);
        assert_eq!(only_mcp[0].kind, "mcp_call");

        let from_a = store
            .find_edges_filtered(Some("a"), None, None, None)
            .unwrap();
        assert_eq!(from_a.len(), 2);
    }

    #[test]
    fn search_fts_returns_hits_with_snippet() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer
            .insert_project(
                snap,
                "Maestro",
                "./Maestro",
                "python",
                r#"{"declared_name":"maestro orchestrator"}"#,
            )
            .unwrap();
        writer.rebuild_search_fts(snap).unwrap();
        writer.commit().unwrap();

        let hits = store.search_fts("orchestrator", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity_id, pid);
        assert!(hits[0].snippet.contains("orchestrator"));
    }

    #[test]
    fn changelog_paginated_respects_limit_and_kind_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        writer
            .insert_change_log(snap, "ts", "project", 1, "added", None, Some("{}"))
            .unwrap();
        writer
            .insert_change_log(snap, "ts", "edge", 1, "added", None, Some("{}"))
            .unwrap();
        writer
            .insert_change_log(snap, "ts", "contract", 1, "added", None, Some("{}"))
            .unwrap();
        writer.commit().unwrap();

        let all = store.changelog_paginated(None, None, 100).unwrap();
        assert_eq!(all.len(), 3);

        let only_projects = store
            .changelog_paginated(None, Some("project"), 100)
            .unwrap();
        assert_eq!(only_projects.len(), 1);

        let limited = store.changelog_paginated(None, None, 2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn schema_v5_creates_search_fts() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'search_fts%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"search_fts".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn search_fts_accepts_inserts_and_returns_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
                 VALUES ('project', 1, 1, 'Maestro', 'DAG orchestrator and runtime')",
                [],
            )
            .unwrap();
        let n: i64 = store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'orchestrator'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn find_edges_with_status_distinguishes_added_removed_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        // Snapshot 1: 2 projects + 1 edge (older).
        let (pid_a, pid_b, eid_old) = {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            let a = writer
                .insert_project(snap, "alpha", "./alpha", "python", "{}")
                .unwrap();
            let b = writer
                .insert_project(snap, "beta", "./beta", "python", "{}")
                .unwrap();
            let e = writer
                .insert_edge(
                    snap,
                    "package_dep",
                    "project",
                    a,
                    "project",
                    b,
                    "{}",
                    "h_old",
                )
                .unwrap();
            writer.commit().unwrap();
            (a, b, e)
        };

        // Snapshot 2: keep old edge alive + add a new mcp_call edge.
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer.touch_project(pid_a, snap, None).unwrap();
            writer.touch_project(pid_b, snap, None).unwrap();
            writer.touch_edge(eid_old, snap, None).unwrap();
            let _ = writer
                .insert_edge(
                    snap,
                    "mcp_call",
                    "project",
                    pid_a,
                    "project",
                    pid_b,
                    r#"{"tool":"t"}"#,
                    "h_new",
                )
                .unwrap();
            writer.commit().unwrap();
        }

        // Snapshot 3: drop the new mcp_call (don't touch it). Keep package_dep alive.
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts3", "/m", None, "0.1.0").unwrap();
            writer.touch_project(pid_a, snap, None).unwrap();
            writer.touch_project(pid_b, snap, None).unwrap();
            writer.touch_edge(eid_old, snap, None).unwrap();
            writer.commit().unwrap();
        }

        // Diff since snapshot 1.
        let diff = store.find_edges_with_status_since(1).unwrap();
        let statuses: std::collections::HashMap<String, String> = diff
            .iter()
            .map(|d| (d.kind.clone(), d.status.clone()))
            .collect();

        assert_eq!(
            statuses.get("package_dep"),
            Some(&"unchanged".to_string()),
            "package_dep first_seen=1 last_seen=3 (max) → unchanged"
        );
        assert_eq!(
            statuses.get("mcp_call"),
            Some(&"removed".to_string()),
            "mcp_call first_seen=2 last_seen=2 < 3 → removed"
        );
    }

    #[test]
    fn schema_v7_creates_module_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"modules".to_string()));
        assert!(names.contains(&"public_symbols".to_string()));
        assert!(names.contains(&"internal_imports".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn schema_v8_creates_symbol_refs_table() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"cross_project_symbol_refs".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn schema_v9_creates_drift_findings_table() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"drift_findings".to_string()));
        assert_eq!(store.schema_version().unwrap(), 9);
    }

    #[test]
    fn drift_findings_kind_check_rejects_bad_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        // Seed a snapshot + project so FK references resolve. Use SnapshotWriter
        // (existing M1 API) — do NOT directly INSERT into snapshots/projects (their
        // column schema differs from what the plan draft showed).
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer
            .insert_project(snap, "p", "./p", "python", "{}")
            .unwrap();
        writer.commit().unwrap();

        let err = store.connection().execute(
            "INSERT INTO drift_findings (project_id, kind, entity_kind, entity_name,
             source_path, source_line, confidence, first_seen, last_seen)
             VALUES (?, 'bogus', 'public_symbol', 'X', 'r.md', 1, 'high', ?, ?)",
            rusqlite::params![pid, snap, snap],
        );
        assert!(err.is_err(), "CHECK constraint should reject 'bogus' kind");
    }
}
