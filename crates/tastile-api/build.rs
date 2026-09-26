//! Build-time validation of the pinned OpenAPI contract.
//!
//! This `build.rs` reads `openapi/openapi.yaml` (the submodule pinned at the
//! revision recorded in `.gitmodules` and the working-tree gitlink) and runs
//! sanity checks before the rest of the crate is compiled:
//!
//! 1. The YAML parses as a single document.
//! 2. The OpenAPI version is `3.1.x`.
//! 3. The `info.title` / `info.version` fields are present.
//! 4. The `paths` map is non-empty and uses `/v1/` prefixes.
//! 5. Each `operationId` referenced by our typed client surface still exists
//!    in the pinned spec (drift gate foundation).
//!
//! Failure here breaks `cargo build`, which is intentional: a submodule bump
//! that breaks the wire contract must not silently compile.
//!
//! Run `mise run check-openapi-drift` for the deeper semantic drift check
//! (Rust type field coverage).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct OpenApiDoc {
    openapi: String,
    info: Info,
    paths: serde_yaml::Mapping,
}

#[derive(Debug, Deserialize)]
struct Info {
    title: String,
    version: String,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels up from crates/tastile-api")
        .to_path_buf();
    let spec_path = workspace_root.join("openapi").join("openapi.yaml");

    println!("cargo:rerun-if-changed={}", spec_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root.join(".gitmodules").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root.join(".git").display()
    );

    let yaml = match fs::read_to_string(&spec_path) {
        Ok(s) => s,
        Err(e) => {
            emit_error(&format!(
                "tastile-api: cannot read pinned OpenAPI spec at {}: {e}. \
                 Did you forget to `git submodule update --init --recursive`?",
                spec_path.display()
            ));
            return;
        }
    };

    let doc: OpenApiDoc = match serde_yaml::from_str(&yaml) {
        Ok(d) => d,
        Err(e) => {
            emit_error(&format!(
                "tastile-api: pinned OpenAPI spec is not parseable: {e}"
            ));
            return;
        }
    };

    if !doc.openapi.starts_with("3.1") {
        emit_error(&format!(
            "tastile-api: expected OpenAPI 3.1.x, got `{}`",
            doc.openapi
        ));
        return;
    }

    if doc.info.title.trim().is_empty() || doc.info.version.trim().is_empty() {
        emit_error("tastile-api: pinned OpenAPI spec is missing info.title or info.version");
        return;
    }

    if doc.paths.is_empty() {
        emit_error("tastile-api: pinned OpenAPI spec has no paths");
        return;
    }

    // Require every /v1/* path key to live under `paths:` and to start with `/v1/`.
    for key in doc.paths.keys() {
        let key_str = match key.as_str() {
            Some(s) => s,
            None => {
                emit_error("tastile-api: non-string path key in pinned OpenAPI spec");
                return;
            }
        };
        if !key_str.starts_with("/v1/") {
            emit_error(&format!(
                "tastile-api: pinned OpenAPI spec path `{key_str}` does not start with `/v1/`"
            ));
            return;
        }
    }

    // Operation-id drift gate: every operation our typed client claims to
    // expose must still exist in the pinned spec.
    let operations = collect_operation_ids(&doc);
    let required = [
        "list_tiles",
        "list_pending_prompts",
        "resolve_prompt",
        "list_source_tiles",
        "get_source_tile",
        "cancel_source_tile",
        "signout",
        "request_prompt",
    ];
    let mut missing: Vec<&str> = Vec::new();
    for op in required {
        if !operations.contains(op) {
            missing.push(op);
        }
    }
    if !missing.is_empty() {
        emit_error(&format!(
            "tastile-api: pinned OpenAPI spec is missing operations required by the typed \
             client: {missing:?}. Bump the API or update `crates/tastile-api` to drop the \
             dependency."
        ));
        return;
    }

    // Surface the pinned spec version as a compile-time env so binaries can
    // print `tastile --version` accurately.
    println!(
        "cargo:rustc-env=TASTILE_API_OPENAPI_VERSION={}",
        doc.info.version
    );
    println!(
        "cargo:rustc-env=TASTILE_API_OPENAPI_TITLE={}",
        doc.info.title
    );
}

fn collect_operation_ids(doc: &OpenApiDoc) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (_path, methods) in doc.paths.iter() {
        let Some(methods) = methods.as_mapping() else {
            continue;
        };
        for (_verb, op) in methods.iter() {
            let Some(op) = op.as_mapping() else {
                continue;
            };
            if let Some(id) = op.get("operationId").and_then(|v| v.as_str()) {
                out.insert(id.to_string());
            }
        }
    }
    out
}

fn emit_error(msg: &str) {
    // cargo:error= terminates the build with a clear message.
    println!("cargo:error={msg}");
    // Also panic so the build script exits non-zero if the `cargo:error=`
    // line is ignored for any reason.
    panic!("{msg}");
}

// Silence the dead-code warning for the helper used only by some compiler
// versions that flag Path even though it is used through ancestors().
#[allow(dead_code)]
fn _force_path_use(p: &Path) -> &Path {
    p
}
