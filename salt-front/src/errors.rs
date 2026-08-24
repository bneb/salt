//! Canonical diagnostic codes for user-facing compiler output.
//!
//! This module is the single source of truth for the [E###] codes printed
//! on hard failures and the [W###] codes printed for recoverable warnings.
//! `saltc --explain <code>` renders the texts in [`EXPLANATIONS`].
//!
//! | Code | Category              | Emitted when                                     |
//! |------|-----------------------|--------------------------------------------------|
//! | E001 | File I/O              | a source/output/SIR file cannot be read/written   |
//! | E002 | Syntax                | source fails to parse or uses invalid syntax      |
//! | E003 | Compilation           | comptime or MLIR lowering fails (CLI banner)      |
//! | E004 | CLI usage             | missing or unknown flags/arguments                |
//! | E005 | Binary synthesis      | MLIR-to-native-binary pipeline fails              |
//! | E006 | Object compilation    | MLIR-to-object pipeline fails                     |
//! | E007 | Internal compiler err | compiler bug or disabled safety flag              |
//! | E008 | Imports/modules       | imported module cannot be resolved (fatal path)   |
//! | E009 | Verification          | a Z3 contract/invariant/ownership proof fails     |
//! | E010 | Target triple         | unknown --target value                            |
//! | E011 | Deferred policy       | --deny-deferred sees deferred checks              |
//!
//! Compile-stage failures print an [E003] banner followed by the root
//! cause; that cause may carry its own refinement code ([E002],
//! [E009], [E011]). Warnings reuse the domain number with a W prefix
//! and never abort compilation (W008: unresolvable import).

/// Every stable error code, in ascending order. Keep in sync with
/// [`EXPLANATIONS`]; the tests below enforce that sync.
pub const ALL_CODES: [&str; 11] = [
    "E001", "E002", "E003", "E004", "E005",
    "E006", "E007", "E008", "E009", "E010", "E011",
];

/// `(code, explanation)` pairs rendered by `saltc --explain <code>`.
pub const EXPLANATIONS: [(&str, &str); 11] = [
    ("E001", "\
[E001] File I/O Error
  The compiler could not read or write a file. This usually means:
  - The source file does not exist at the specified path
  - The output directory is not writable
  - The file is not valid UTF-8 text

  Example: `saltc nonexistent.salt -o out.mlir`"),
    ("E002", "\
[E002] Syntax Error
  The source code could not be parsed. Check for:
  - Missing semicolons, braces, or parentheses
  - Invalid Salt syntax

  Example: a missing closing brace or an unclosed string literal."),
    ("E003", "\
[E003] Compilation Error
  The compiler could not generate valid MLIR from the source code.
  This can be caused by type errors, unresolved symbols, or verification failures.

  Example: Z3 contract violation like calling safe_div(100, 0) with requires(b != 0)."),
    ("E004", "\
[E004] CLI Usage Error
  An invalid flag or argument was provided on the command line.
  Run `saltc --help` for a full list of options.

  Example: `saltc --invalid-flag source.salt` or missing output path."),
    ("E005", "\
[E005] Binary Synthesis Error
  The MLIR-to-native-binary pipeline failed. This usually means:
  - LLVM tools (mlir-opt, mlir-translate, llc) are not installed
  - The target triple is not supported
  - A linker or runtime object is missing

  Example: running `saltc --target keuos` without the KeuOS runtime toolchain."),
    ("E006", "\
[E006] Object Compilation Error
  The MLIR-to-object-file pipeline failed.
  Check that LLVM toolchain is correctly installed.

  Example: missing LLVM tools (llc, mlir-translate) in PATH."),
    ("E007", "\
[E007] Internal Compiler Error
  This is a bug in the Salt compiler. Please report it at:
  https://github.com/kevin/salt/issues

  Please report this bug with the source file and the exact saltc command."),
    ("E008", "\
[E008] Import / Module Error
  An imported module could not be found or parsed.

  Example: importing a module that does not exist or has a misspelled type name."),
    ("E009", "\
[E009] Verification Error
  A Z3 contract or ownership verification check failed.

  Example: a Z3 contract violation such as dividing by zero without a precondition."),
    ("E010", "\
[E010] Target Triple Error
  The specified target triple is not recognized or supported.
  Supported targets: macos, linux-arm64, keuos, keuos-x86_64

  Example: `saltc --target unsupported-target source.salt`."),
    ("E011", "\
[E011] Deferred Verification Policy Error
  --deny-deferred was passed and at least one Z3 check could not be
  statically proven; it would have been deferred to a runtime check.

  Fix: add loop invariants, strengthen preconditions, or remove --deny-deferred."),
];

/// Prefix msg with a bracketed diagnostic code, e.g. "[E004] unknown flag".
pub fn coded(code: &str, msg: impl std::fmt::Display) -> String {
    format!("[{}] {}", code, msg)
}

/// The explanation registered for code, if it is a known code.
pub fn explanation(code: &str) -> Option<&'static str> {
    EXPLANATIONS.iter().find(|(c, _)| *c == code).map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explanations_cover_all_codes_in_order() {
        assert_eq!(ALL_CODES.len(), EXPLANATIONS.len());
        for (i, code) in ALL_CODES.iter().enumerate() {
            assert_eq!(EXPLANATIONS[i].0, *code, "code order drifted at {}", code);
        }
    }

    #[test]
    fn codes_are_sorted_and_unique() {
        for pair in ALL_CODES.windows(2) {
            assert!(pair[0] < pair[1], "codes must ascend: {} vs {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn every_explanation_opens_with_its_own_code() {
        for (code, text) in EXPLANATIONS.iter() {
            let prefix = format!("[{}]", code);
            assert!(text.starts_with(&prefix), "{} explanation lacks {}", code, prefix);
        }
    }

    #[test]
    fn coded_prefixes_message_with_bracketed_code() {
        assert_eq!(coded("E004", "unknown argument"), "[E004] unknown argument");
        assert_eq!(coded("W008", ""), "[W008] ");
    }

    #[test]
    fn explanation_lookup_round_trips() {
        for code in ALL_CODES.iter() {
            assert!(explanation(code).is_some(), "{} has no explanation", code);
        }
        assert_eq!(explanation("E001"), Some(EXPLANATIONS[0].1));
        assert!(explanation("E999").is_none());
        assert!(explanation("").is_none());
    }
}
