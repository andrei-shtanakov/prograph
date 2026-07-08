//! Contract-file scanner — finds JSON Schema, OpenAPI, and .proto files inside a project
//! and classifies them. Pure file-system + sniffing; no AST.

use std::path::Path;

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::facts::{ContractFile, ContractKind};

/// Walk `project_root` for contract files. Returns the list of detected contracts.
/// Hidden + build-artefact dirs are skipped.
pub fn scan(project_root: &Path) -> Vec<ContractFile> {
    let mut out = Vec::new();

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(ext, "json" | "yaml" | "yml" | "proto") {
            continue;
        }

        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let kind_and_id = classify(ext, &contents);
        let Some((kind, declared_id)) = kind_and_id else {
            continue;
        };

        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(contents.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        out.push(ContractFile {
            rel_path,
            kind,
            declared_id,
            content_hash,
        });
    }

    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

/// Decide whether a file is a contract and (if so) extract its declared id.
fn classify(ext: &str, contents: &str) -> Option<(ContractKind, Option<String>)> {
    match ext {
        "proto" => {
            let declared_id = extract_proto_package(contents);
            Some((ContractKind::Proto, declared_id))
        }
        "json" => classify_json(contents),
        "yaml" | "yml" => classify_yaml(contents),
        _ => None,
    }
}

fn classify_json(contents: &str) -> Option<(ContractKind, Option<String>)> {
    let v: serde_json::Value = match serde_json::from_str(contents) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let obj = v.as_object()?;

    // OpenAPI: top-level "openapi" or "swagger" key
    if obj.contains_key("openapi") || obj.contains_key("swagger") {
        let title = obj
            .get("info")
            .and_then(|i| i.as_object())
            .and_then(|i| i.get("title"))
            .and_then(|t| t.as_str())
            .map(String::from);
        return Some((ContractKind::OpenApi, title));
    }

    // JSON Schema: $schema OR $id at top level
    if obj.contains_key("$schema") || obj.contains_key("$id") {
        let id = obj.get("$id").and_then(|v| v.as_str()).map(String::from);
        return Some((ContractKind::JsonSchema, id));
    }

    None
}

fn classify_yaml(contents: &str) -> Option<(ContractKind, Option<String>)> {
    let v: serde_yaml::Value = serde_yaml::from_str(contents).ok()?;
    let map = v.as_mapping()?;

    let has_openapi = map.iter().any(|(k, _)| {
        k.as_str()
            .map(|s| s == "openapi" || s == "swagger")
            .unwrap_or(false)
    });
    if has_openapi {
        let title = map
            .iter()
            .find(|(k, _)| k.as_str() == Some("info"))
            .and_then(|(_, v)| v.as_mapping())
            .and_then(|info| info.iter().find(|(k, _)| k.as_str() == Some("title")))
            .and_then(|(_, v)| v.as_str())
            .map(String::from);
        return Some((ContractKind::OpenApi, title));
    }
    None
}

fn extract_proto_package(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            let name = rest.trim_end_matches(';').trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_json_schema() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("schema.json"),
            r#"{"$id": "https://example.org/schemas/obs-v1", "$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object"}"#,
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::JsonSchema);
        assert_eq!(
            contracts[0].declared_id.as_deref(),
            Some("https://example.org/schemas/obs-v1")
        );
    }

    #[test]
    fn classifies_openapi_json() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("api.json"),
            r#"{"openapi": "3.0.0", "info": {"title": "My API", "version": "1.0"}}"#,
        )
        .unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::OpenApi);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("My API"));
    }

    #[test]
    fn classifies_openapi_yaml() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("api.yaml"),
            "openapi: 3.0.0\ninfo:\n  title: Y API\n  version: 1.0\n",
        )
        .unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::OpenApi);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("Y API"));
    }

    #[test]
    fn classifies_proto_with_package() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("svc.proto"),
            "syntax = \"proto3\";\npackage my.service.v1;\nmessage X {}\n",
        )
        .unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::Proto);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("my.service.v1"));
    }

    #[test]
    fn skips_plain_json() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("config.json"),
            r#"{"foo": "bar"}"#, // no $schema, no $id, no openapi
        )
        .unwrap();
        let contracts = scan(dir.path());
        assert!(
            contracts.is_empty(),
            "plain JSON without contract markers must not be classified"
        );
    }

    #[test]
    fn content_hash_is_stable() {
        let dir = TempDir::new().unwrap();
        let contents = r#"{"$schema": "x", "type": "object"}"#;
        fs::write(dir.path().join("a.json"), contents).unwrap();
        let h1 = scan(dir.path())[0].content_hash.clone();

        let dir2 = TempDir::new().unwrap();
        fs::write(dir2.path().join("a.json"), contents).unwrap();
        let h2 = scan(dir2.path())[0].content_hash.clone();

        assert_eq!(h1, h2, "identical content must produce identical hash");
    }

    #[test]
    fn finds_contracts_under_subdirs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("schemas/v1")).unwrap();
        fs::write(
            dir.path().join("schemas/v1/obs.json"),
            r#"{"$id": "obs-v1", "type": "object"}"#,
        )
        .unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].rel_path, "schemas/v1/obs.json");
    }
}
