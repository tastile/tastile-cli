use std::collections::BTreeSet;

/// Required fields of an envelope's payload schema, including referenced
/// alternatives in nullable `oneOf` payloads.
pub fn payload_required_fields(
    envelope: &str,
    schemas: &serde_yaml::Mapping,
) -> Option<BTreeSet<String>> {
    let schema = schemas
        .get(envelope)
        .and_then(|schema| schema.as_mapping())?;
    let payload = schema
        .get("properties")
        .and_then(|properties| properties.as_mapping())
        .and_then(|properties| properties.get("payload"))?;

    let mut required = BTreeSet::new();
    let mut visited = BTreeSet::new();
    collect_payload_required_fields(payload, schemas, &mut required, &mut visited);
    Some(required)
}

fn collect_payload_required_fields(
    schema: &serde_yaml::Value,
    schemas: &serde_yaml::Mapping,
    required: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) {
    let Some(schema) = schema.as_mapping() else {
        return;
    };

    if let Some(fields) = schema
        .get("required")
        .and_then(|fields| fields.as_sequence())
    {
        required.extend(
            fields
                .iter()
                .filter_map(|field| field.as_str().map(str::to_owned)),
        );
    }

    if let Some(reference) = schema.get("$ref").and_then(|value| value.as_str()) {
        let name = reference.rsplit('/').next().unwrap_or(reference);
        if visited.insert(name.to_owned()) {
            if let Some(referenced_schema) = schemas.get(name) {
                collect_payload_required_fields(referenced_schema, schemas, required, visited);
            }
        }
    }

    for composition in ["allOf", "anyOf", "oneOf"] {
        if let Some(variants) = schema
            .get(composition)
            .and_then(|variants| variants.as_sequence())
        {
            for variant in variants {
                collect_payload_required_fields(variant, schemas, required, visited);
            }
        }
    }
}

/// Return spec-required fields the typed client has not modelled as required
/// or explicitly known-optional.
pub fn unmodelled_required_fields(
    spec_required: &BTreeSet<String>,
    contract_required: &[&str],
    known_optional: &[&str],
) -> Vec<String> {
    let allowed = contract_required
        .iter()
        .chain(known_optional)
        .copied()
        .collect::<BTreeSet<_>>();
    spec_required
        .iter()
        .filter(|field| !allowed.contains(field.as_str()))
        .cloned()
        .collect()
}

/// Request envelopes whose currently optional revision is populated by the
/// CLI and may become required in a future published OpenAPI revision.
pub fn known_optional_envelope_fields(schema: &str) -> &'static [&'static str] {
    match schema {
        "UpdateSourceTileRequest" | "CancelSourceTileRequest" | "ReflowSourceTileRequest" => {
            &["expected_revision"]
        }
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        known_optional_envelope_fields, payload_required_fields, unmodelled_required_fields,
    };

    #[test]
    fn spec_required_additions_must_be_contract_required_or_known_optional() {
        let spec_required = [
            "idempotency_key",
            "payload",
            "expected_revision",
            "new_field",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

        assert_eq!(
            unmodelled_required_fields(
                &spec_required,
                &["idempotency_key", "payload"],
                &["expected_revision"],
            ),
            vec!["new_field".to_string()]
        );
    }

    #[test]
    fn reflow_expected_revision_is_known_optional_until_the_pin_is_bumped() {
        assert_eq!(
            known_optional_envelope_fields("ReflowSourceTileRequest"),
            &["expected_revision"]
        );
    }

    #[test]
    fn payload_required_fields_are_extracted_for_empty_and_nullable_contracts() {
        let document: serde_yaml::Value = serde_yaml::from_str(
            r##"
components:
  schemas:
    RequestPromptRequest:
      type: object
      properties:
        payload:
          $ref: '#/components/schemas/RequestPromptPayloadSchema'
    RequestPromptPayloadSchema:
      type: object
      required: [kind, newly_required]
      properties:
        kind: {type: integer}
        newly_required: {type: integer}
    ExecutionLifecycleRequest:
      type: object
      properties:
        payload:
          oneOf:
            - type: 'null'
            - $ref: '#/components/schemas/EmptyPayloadSchema'
    EmptyPayloadSchema:
      type: object
      required: [new_lifecycle_field]
      properties:
        new_lifecycle_field: {type: integer}
"##,
        )
        .expect("fixture YAML");
        let schemas = document["components"]["schemas"]
            .as_mapping()
            .expect("schema mapping");

        let prompt_required = payload_required_fields("RequestPromptRequest", schemas)
            .expect("prompt request has a payload schema");
        assert_eq!(
            unmodelled_required_fields(&prompt_required, &[], &[]),
            vec!["kind".to_string(), "newly_required".to_string()]
        );
        let lifecycle_required = payload_required_fields("ExecutionLifecycleRequest", schemas)
            .expect("lifecycle request has a nullable payload schema");
        assert_eq!(
            unmodelled_required_fields(&lifecycle_required, &[], &[]),
            vec!["new_lifecycle_field".to_string()]
        );
    }
}
