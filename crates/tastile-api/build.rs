//! Build-time validation of the pinned OpenAPI contract.
//!
//! Reads `openapi/openapi.yaml` from the workspace-root submodule and runs a
//! structural drift gate: every operation the typed client exposes must still
//! exist with the right method, path, request body schema, response schema,
//! path parameters, and required body envelope fields. A submodule bump that
//! breaks the wire contract fails the build here, before the rest of the
//! crate compiles.
//!
//! Run `scripts/check-openapi-drift.sh` for the same checks expressed as a
//! shell script so they can also run in CI without Rust toolchain.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct OpenApiDoc {
    openapi: String,
    info: Info,
    paths: serde_yaml::Mapping,
    #[serde(default, rename = "components")]
    components: Option<Components>,
}

#[derive(Debug, Deserialize)]
struct Info {
    title: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct Components {
    #[serde(default)]
    schemas: serde_yaml::Mapping,
}

#[derive(Debug, Clone, Copy)]
enum ResponseSpec {
    /// No 200 body expected.
    None,
    /// Bare `$ref` to a named schema.
    Ref(&'static str),
    /// `items: $ref ...` — array of the named schema.
    ArrayOf(&'static str),
    /// `schema: {}` — empty schema, content is opaque to the client.
    Empty,
}

#[derive(Debug, Clone, Copy)]
struct OperationContract {
    operation_id: &'static str,
    method: &'static str,
    path: &'static str,
    /// Top-level request body schema ref (e.g. `"CreateSourceTileRequest"`).
    request_body_schema: Option<&'static str>,
    /// `200` response shape.
    response_200: ResponseSpec,
    /// Path parameter names in declaration order.
    path_params: &'static [&'static str],
    /// Required top-level fields of the request body envelope
    /// (excluding `payload`, which is verified separately via
    /// `payload_required_fields`).
    required_envelope_fields: &'static [&'static str],
    /// Required top-level fields of the `payload` object (if any).
    payload_required_fields: &'static [&'static str],
}

/// Typed surface contract. The list of `OperationContract` rows is the source
/// of truth for what the CLI exercises from the pinned spec.
const CONTRACTS: &[OperationContract] = &[
    OperationContract {
        operation_id: "list_tiles",
        method: "get",
        path: "/v1/tiles",
        request_body_schema: None,
        response_200: ResponseSpec::ArrayOf("TileListView"),
        path_params: &[],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "list_pending_prompts",
        method: "get",
        path: "/v1/prompts/pending",
        request_body_schema: None,
        response_200: ResponseSpec::ArrayOf("PromptView"),
        path_params: &[],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "request_prompt",
        method: "post",
        path: "/v1/prompts",
        request_body_schema: Some("RequestPromptRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &[],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "resolve_prompt",
        method: "post",
        path: "/v1/prompts/{prompt_id}/resolve",
        request_body_schema: Some("ResolvePromptRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["prompt_id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["resolution"],
    },
    OperationContract {
        operation_id: "respond_startup_recovery",
        method: "post",
        path: "/v1/prompts/startup-recovery",
        request_body_schema: Some("StartupRecoveryRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &[],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["prompt_id", "resolution"],
    },
    OperationContract {
        operation_id: "list_source_tiles",
        method: "get",
        path: "/v1/source-tiles",
        request_body_schema: None,
        response_200: ResponseSpec::ArrayOf("SourceTileRead"),
        path_params: &[],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "create_source_tile",
        method: "post",
        path: "/v1/source-tiles",
        request_body_schema: Some("CreateSourceTileRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &[],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["tile", "plan", "flows", "schedule", "horizon"],
    },
    OperationContract {
        operation_id: "get_source_tile",
        method: "get",
        path: "/v1/source-tiles/{id}",
        request_body_schema: None,
        response_200: ResponseSpec::Ref("SourceTileDetailRead"),
        path_params: &["id"],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "update_source_tile",
        method: "put",
        path: "/v1/source-tiles/{id}",
        request_body_schema: Some("UpdateSourceTileRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["tile", "plan", "flows", "schedule", "horizon"],
    },
    OperationContract {
        operation_id: "cancel_source_tile",
        method: "post",
        path: "/v1/source-tiles/{id}/cancel",
        request_body_schema: Some("CancelSourceTileRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "get_source_tile_completion",
        method: "get",
        path: "/v1/source-tiles/{id}/completion",
        request_body_schema: None,
        response_200: ResponseSpec::Empty,
        path_params: &["id"],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "list_source_tile_placements",
        method: "get",
        path: "/v1/source-tiles/{id}/placements",
        request_body_schema: None,
        response_200: ResponseSpec::ArrayOf("PlacementTileRead"),
        path_params: &["id"],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "reflow_source_tile",
        method: "post",
        path: "/v1/source-tiles/{id}/reflow",
        request_body_schema: Some("ReflowSourceTileRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["range"],
    },
    OperationContract {
        operation_id: "start_execution",
        method: "post",
        path: "/v1/placements/{id}/executions",
        request_body_schema: Some("StartExecutionRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["placement_id"],
    },
    OperationContract {
        operation_id: "pause_execution",
        method: "post",
        path: "/v1/executions/{id}/pause",
        request_body_schema: Some("ExecutionLifecycleRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key"],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "resume_execution",
        method: "post",
        path: "/v1/executions/{id}/resume",
        request_body_schema: Some("ExecutionLifecycleRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key"],
        payload_required_fields: &[],
    },
    OperationContract {
        operation_id: "finish_execution",
        method: "post",
        path: "/v1/executions/{id}/finish",
        request_body_schema: Some("FinishExecutionRequest"),
        response_200: ResponseSpec::Ref("CommandResponse"),
        path_params: &["id"],
        required_envelope_fields: &["idempotency_key", "payload"],
        payload_required_fields: &["kind"],
    },
    OperationContract {
        operation_id: "signout",
        method: "post",
        path: "/v1/auth/signout",
        request_body_schema: None,
        response_200: ResponseSpec::None,
        path_params: &[],
        required_envelope_fields: &[],
        payload_required_fields: &[],
    },
];

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

    let schemas = doc
        .components
        .as_ref()
        .map(|c| &c.schemas)
        .cloned()
        .unwrap_or_default();

    for contract in CONTRACTS {
        check_contract(contract, &doc.paths, &schemas);
    }

    let observed = observe_spec(&doc.paths);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let _ = fs::write(
        out_dir.join("openapi_observed.json"),
        serde_json::to_string_pretty(&observed).expect("serialize openapi observed artifact"),
    );

    println!(
        "cargo:rustc-env=TASTILE_API_OPENAPI_VERSION={}",
        doc.info.version
    );
    println!(
        "cargo:rustc-env=TASTILE_API_OPENAPI_TITLE={}",
        doc.info.title
    );
}

fn check_contract(
    contract: &OperationContract,
    paths: &serde_yaml::Mapping,
    schemas: &serde_yaml::Mapping,
) {
    let path_item = match paths.get(contract.path) {
        Some(v) => v,
        None => {
            emit_error(&format!(
                "tastile-api: contract drift: path `{}` for operation `{}` is missing from the pinned spec",
                contract.path, contract.operation_id
            ));
            return;
        }
    };
    let path_item = match path_item.as_mapping() {
        Some(m) => m,
        None => {
            emit_error(&format!(
                "tastile-api: pinned spec path `{}` is not a mapping",
                contract.path
            ));
            return;
        }
    };
    let method_item = match path_item.get(contract.method) {
        Some(v) => v,
        None => {
            emit_error(&format!(
                "tastile-api: contract drift: method `{}` on path `{}` (op `{}`) is missing",
                contract.method, contract.path, contract.operation_id
            ));
            return;
        }
    };
    let method_item = match method_item.as_mapping() {
        Some(m) => m,
        None => {
            emit_error(&format!(
                "tastile-api: pinned spec method item for `{} {}` is not a mapping",
                contract.method, contract.path
            ));
            return;
        }
    };

    match method_item.get("operationId").and_then(|v| v.as_str()) {
        Some(id) if id == contract.operation_id => {}
        Some(id) => {
            emit_error(&format!(
                "tastile-api: contract drift: operationId for `{} {}` is `{}`, expected `{}`",
                contract.method, contract.path, id, contract.operation_id
            ));
        }
        None => {
            emit_error(&format!(
                "tastile-api: contract drift: operationId for `{} {}` is missing",
                contract.method, contract.path
            ));
        }
    }

    // Path parameters: declared on the path item OR on the method itself.
    let mut path_params: Vec<String> = path_item
        .get("parameters")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|p| {
                    let m = p.as_mapping()?;
                    let name = m.get("name").and_then(|n| n.as_str())?;
                    let location = m.get("in").and_then(|l| l.as_str())?;
                    if location == "path" {
                        Some(name.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let method_path_params: Vec<String> = method_item
        .get("parameters")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|p| {
                    let m = p.as_mapping()?;
                    let name = m.get("name").and_then(|n| n.as_str())?;
                    let location = m.get("in").and_then(|l| l.as_str())?;
                    if location == "path" {
                        Some(name.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    for p in &method_path_params {
        if !path_params.iter().any(|x| x == p) {
            path_params.push(p.clone());
        }
    }
    let expected_params: Vec<String> = contract.path_params.iter().map(|s| s.to_string()).collect();
    if expected_params != path_params {
        emit_error(&format!(
            "tastile-api: contract drift: path parameters for `{} {}` are {:?}, expected {:?}",
            contract.method, contract.path, path_params, expected_params
        ));
        return;
    }

    // Method-level path parameters must also appear on the path item.
    // (Already merged above.)

    // Request body schema.
    match contract.request_body_schema {
        Some(expected) => {
            let body = method_item.get("requestBody").and_then(|b| b.as_mapping());
            let body_schema = body
                .and_then(|b| b.get("content"))
                .and_then(|c| c.as_mapping())
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.as_mapping())
                .and_then(|j| j.get("schema"))
                .and_then(|s| s.as_mapping())
                .and_then(|s| s.get("$ref"))
                .and_then(|r| r.as_str());
            let actual = body_schema.map(|s| s.rsplit('/').next().unwrap_or(s).to_string());
            match actual {
                Some(ref a) if a == expected => {}
                Some(a) => {
                    emit_error(&format!(
                        "tastile-api: contract drift: request body schema for `{} {}` is `{}`, expected `{}`",
                        contract.method, contract.path, a, expected
                    ));
                    return;
                }
                None => {
                    emit_error(&format!(
                        "tastile-api: contract drift: request body for `{} {}` is missing or unref'd, expected `{}`",
                        contract.method, contract.path, expected
                    ));
                    return;
                }
            }

            let body_required = schema_required_fields(expected, schemas);
            for required in contract.required_envelope_fields {
                if !body_required.contains(*required) {
                    emit_error(&format!(
                        "tastile-api: contract drift: request body for `{} {}` (schema `{}`) is missing required field `{}`",
                        contract.method, contract.path, expected, required
                    ));
                    return;
                }
            }
            if !contract.payload_required_fields.is_empty() {
                let payload_required = payload_required_fields(expected, schemas);
                for required in contract.payload_required_fields {
                    if !payload_required.contains(*required) {
                        emit_error(&format!(
                            "tastile-api: contract drift: payload for `{} {}` is missing required field `{}`",
                            contract.method, contract.path, required
                        ));
                        return;
                    }
                }
            }
        }
        None => {
            if method_item.get("requestBody").is_some() {
                emit_error(&format!(
                    "tastile-api: contract drift: `{} {}` is expected to have no request body but the pinned spec declares one",
                    contract.method, contract.path
                ));
                return;
            }
        }
    }

    // Response 200.
    check_response(contract, method_item);
}

fn check_response(contract: &OperationContract, method_item: &serde_yaml::Mapping) {
    let resp200 = method_item
        .get("responses")
        .and_then(|r| r.as_mapping())
        .and_then(|r| r.get("200"))
        .and_then(|r| r.as_mapping());
    match contract.response_200 {
        ResponseSpec::None => {
            if resp200.is_some() {
                // The spec may declare `200` with `schema: {}` (empty body). If
                // we expected no body, any non-empty schema would be drift.
                let schema = resp200
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.as_mapping())
                    .and_then(|c| c.get("application/json"))
                    .and_then(|j| j.as_mapping())
                    .and_then(|j| j.get("schema"));
                if let Some(s) = schema {
                    if !is_empty_schema(s) {
                        emit_error(&format!(
                            "tastile-api: contract drift: `{} {}` is expected to have no 200 body but the pinned spec declares one",
                            contract.method, contract.path
                        ));
                    }
                }
            }
        }
        ResponseSpec::Empty => {
            let schema = resp200
                .and_then(|r| r.get("content"))
                .and_then(|c| c.as_mapping())
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.as_mapping())
                .and_then(|j| j.get("schema"));
            match schema {
                Some(s) if is_empty_schema(s) => {}
                Some(_) => {
                    emit_error(&format!(
                        "tastile-api: contract drift: `{} {}` is expected to return `schema: {{}}` but the pinned spec declares a non-empty schema",
                        contract.method, contract.path
                    ));
                }
                None => {
                    emit_error(&format!(
                        "tastile-api: contract drift: `{} {}` is expected to return `schema: {{}}` but the pinned spec has no 200 body",
                        contract.method, contract.path
                    ));
                }
            }
        }
        ResponseSpec::Ref(expected) => {
            let actual = ref_name(resp200.and_then(|r| r.get("content")));
            match actual {
                Some(ref a) if a == expected => {}
                Some(a) => {
                    emit_error(&format!(
                        "tastile-api: contract drift: response 200 schema for `{} {}` is `{}`, expected `{}`",
                        contract.method, contract.path, a, expected
                    ));
                }
                None => {
                    emit_error(&format!(
                        "tastile-api: contract drift: response 200 schema for `{} {}` is missing or unref'd, expected `{}`",
                        contract.method, contract.path, expected
                    ));
                }
            }
        }
        ResponseSpec::ArrayOf(expected) => {
            let items_ref = resp200
                .and_then(|r| r.get("content"))
                .and_then(|c| c.as_mapping())
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.as_mapping())
                .and_then(|j| j.get("schema"))
                .and_then(|s| s.as_mapping())
                .and_then(|s| s.get("items"))
                .and_then(|i| i.as_mapping())
                .and_then(|i| i.get("$ref"))
                .and_then(|r| r.as_str());
            let actual = items_ref.map(|s| s.rsplit('/').next().unwrap_or(s).to_string());
            match actual {
                Some(ref a) if a == expected => {}
                Some(a) => {
                    emit_error(&format!(
                        "tastile-api: contract drift: response 200 array element for `{} {}` is `{}`, expected `{}`",
                        contract.method, contract.path, a, expected
                    ));
                }
                None => {
                    // Confirm the spec actually declared an array; otherwise we
                    // can't decide whether drift happened.
                    let is_array = resp200
                        .and_then(|r| r.get("content"))
                        .and_then(|c| c.as_mapping())
                        .and_then(|c| c.get("application/json"))
                        .and_then(|j| j.as_mapping())
                        .and_then(|j| j.get("schema"))
                        .and_then(|s| s.as_mapping())
                        .and_then(|s| s.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("array");
                    if is_array {
                        emit_error(&format!(
                            "tastile-api: contract drift: response 200 array element for `{} {}` is unref'd, expected `{}`",
                            contract.method, contract.path, expected
                        ));
                    } else {
                        emit_error(&format!(
                            "tastile-api: contract drift: response 200 for `{} {}` is not an array, expected array of `{}`",
                            contract.method, contract.path, expected
                        ));
                    }
                }
            }
        }
    }
}

fn is_empty_schema(s: &serde_yaml::Value) -> bool {
    let Some(m) = s.as_mapping() else {
        return false;
    };
    m.is_empty()
}

fn schema_required_fields(name: &str, schemas: &serde_yaml::Mapping) -> BTreeSet<String> {
    let Some(schema) = schemas.get(name).and_then(|s| s.as_mapping()) else {
        return BTreeSet::new();
    };
    schema
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn payload_required_fields(envelope: &str, schemas: &serde_yaml::Mapping) -> BTreeSet<String> {
    let Some(schema) = schemas.get(envelope).and_then(|s| s.as_mapping()) else {
        return BTreeSet::new();
    };
    let Some(properties) = schema.get("properties").and_then(|p| p.as_mapping()) else {
        return BTreeSet::new();
    };
    let Some(payload) = properties.get("payload").and_then(|p| p.as_mapping()) else {
        return BTreeSet::new();
    };
    let Some(ref_str) = payload.get("$ref").and_then(|r| r.as_str()) else {
        return BTreeSet::new();
    };
    let payload_schema = ref_str.rsplit('/').next().unwrap_or(ref_str);
    schema_required_fields(payload_schema, schemas)
}

fn observe_spec(paths: &serde_yaml::Mapping) -> serde_json::Value {
    let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (path, methods) in paths.iter() {
        let Some(path_str) = path.as_str() else {
            continue;
        };
        let Some(methods) = methods.as_mapping() else {
            continue;
        };
        let mut method_map: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (verb, op) in methods.iter() {
            let Some(verb_str) = verb.as_str() else {
                continue;
            };
            if !["get", "post", "put", "delete", "patch"].contains(&verb_str) {
                continue;
            }
            let Some(op) = op.as_mapping() else { continue };
            let entry = json!({
                "operationId": op.get("operationId").and_then(|v| v.as_str()),
                "requestBody": ref_name(op.get("requestBody")),
                "responses": {
                    "200": ref_name(
                        op.get("responses")
                            .and_then(|r| r.as_mapping())
                            .and_then(|r| r.get("200"))
                    ),
                },
            });
            method_map.insert(verb_str.to_string(), entry);
        }
        out.insert(
            path_str.to_string(),
            serde_json::Value::Object(method_map.into_iter().collect()),
        );
    }
    serde_json::Value::Object(out.into_iter().collect())
}

fn ref_name(content: Option<&serde_yaml::Value>) -> Option<String> {
    let content = content?.as_mapping()?;
    let app_json = content.get("application/json")?.as_mapping()?;
    let schema = app_json.get("schema")?.as_mapping()?;
    let r = schema.get("$ref").and_then(|r| r.as_str())?;
    Some(r.rsplit('/').next().unwrap_or(r).to_string())
}

fn emit_error(msg: &str) {
    println!("cargo:error={msg}");
    panic!("{msg}");
}

#[allow(dead_code)]
fn _force_path_use(p: &Path) -> &Path {
    p
}
