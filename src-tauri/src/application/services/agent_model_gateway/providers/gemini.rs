pub(super) const GEMINI_NATIVE_PROVIDER: Option<&str> = Some("gemini");
pub(super) const INTERACTIONS_NATIVE_PROVIDER: Option<&str> = Some("gemini_interactions");

pub(super) const SCHEMA_KEYS_TO_REMOVE: &[&str] = &[
    "$schema",
    "$id",
    "$defs",
    "definitions",
    "additionalProperties",
    "patternProperties",
    "unevaluatedProperties",
    "dependencies",
    "dependentRequired",
    "dependentSchemas",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "const",
    "default",
    "examples",
    "title",
];
