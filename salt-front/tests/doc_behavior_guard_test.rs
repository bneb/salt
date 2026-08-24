// Doc-behavior conformance guard (roadmap Sprint 1, workstream 3).
//
// Fails when a public doc comment's behavioral claim drifts from what the
// code does. Each entry pairs a claim, quoted from the module's doc comment,
// with an executable check of that claim. If you change behavior or docs,
// update BOTH sides — this file is the tripwire between them.
//
// To extend: add (claim source location, executable check) pairs for public
// doc comments that promise specific observable behavior.
use saltc::errors::{explanation, ALL_CODES, EXPLANATIONS};
use saltc::grammar::attr::{extract_pulse_hz, extract_yielding_pulse, Attribute};
use saltc::grammar::SaltFn;

fn parse_attr(src: &str) -> Attribute {
    syn::parse_str::<Attribute>(src)
        .unwrap_or_else(|e| panic!("fixture {src:?} must parse as attribute: {e}"))
}

/// attr.rs on extract_pulse_hz: "Examples: @pulse(60) -> Some(60),
/// @pulse(1000) -> Some(1000)".
#[test]
fn pulse_hz_extraction_matches_doc_examples() {
    assert_eq!(extract_pulse_hz(&[parse_attr("@pulse(60)")]), Some(60));
    assert_eq!(extract_pulse_hz(&[parse_attr("@pulse(1000)")]), Some(1000));
}

/// attr.rs on extract_yielding_pulse: "Default pulse is 1024 when @yielding
/// has no argument".
#[test]
fn yielding_default_pulse_matches_doc() {
    assert_eq!(extract_yielding_pulse(&[parse_attr("@yielding")]), Some(1024));
    assert_eq!(extract_yielding_pulse(&[parse_attr("@yielding(2048)")]), Some(2048));
}

/// attr.rs on hash-bracket attributes: "Accepted as an alias for the
/// Salt-native @-form so sources written with familiar repr/string_prefix
/// hash attributes register them instead of silently dropping them."
#[test]
fn hash_bracket_attributes_register_like_at_form() {
    let at_form = syn::parse_str::<SaltFn>("@inline fn f() { }")
        .expect("@-form function must parse");
    let hash_form = syn::parse_str::<SaltFn>("#[inline]\nfn f() { }")
        .expect("hash-bracket function must parse");
    let names = |f: &SaltFn| {
        f.attributes.iter()
            .map(|a| a.name.to_string())
            .collect::<Vec<_>>()
    };
    assert!(names(&at_form).contains(&"inline".to_string()));
    assert_eq!(
        names(&hash_form),
        names(&at_form),
        "hash-bracket alias must register the same attribute name"
    );
}

/// errors.rs module doc: ALL_CODES is "the single source of truth" kept in
/// sync with EXPLANATIONS, each rendered by `saltc --explain <code>`.
#[test]
fn error_code_table_matches_explanations() {
    assert_eq!(ALL_CODES.len(), EXPLANATIONS.len(), "table sizes diverged");
    for code in ALL_CODES {
        let text = explanation(code)
            .unwrap_or_else(|| panic!("{code} documented but has no explanation"));
        let banner = format!("[{code}]");
        assert!(
            text.starts_with(&banner),
            "explanation for {code} must open with its own banner, got: {text:?}"
        );
    }
}
