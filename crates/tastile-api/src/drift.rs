use std::collections::BTreeSet;

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

    use super::{known_optional_envelope_fields, unmodelled_required_fields};

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
}
