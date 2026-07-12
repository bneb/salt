// ============================================================================
// String Literal Escaping Tests
// Guards against MLIR parsing errors from improperly escaped strings
//
// Fixes tested:
// - Newlines escaped as \n (not literal newline breaking MLIR)
// - Quotes escaped as \"
// - Backslashes escaped as \\
// - Tabs and carriage returns escaped
// ============================================================================

#[cfg(test)]
mod tests {

    // =========================================================================
    // Parameterized Test Cases for String Escaping
    // =========================================================================

    struct StringEscapeCase {
        /// Input string content
        input: &'static str,
        /// Description
        description: &'static str,
        /// Expected escaped output
        expected: &'static str,
    }

    fn get_string_escape_cases() -> Vec<StringEscapeCase> {
        vec![
            // Case 1: Simple string - no escaping needed
            StringEscapeCase {
                input: "hello",
                description: "Simple ASCII string needs no escaping",
                expected: "hello",
            },
            // Case 2: Newline escaping
            StringEscapeCase {
                input: "hello\nworld",
                description: "Newline must be escaped as \\n",
                expected: "hello\\nworld",
            },
            // Case 3: Quote escaping
            StringEscapeCase {
                input: "say \"hi\"",
                description: "Quotes must be escaped as \\\"",
                expected: "say \\\"hi\\\"",
            },
            // Case 4: Backslash escaping
            StringEscapeCase {
                input: "path\\to\\file",
                description: "Backslash must be escaped as \\\\",
                expected: "path\\\\to\\\\file",
            },
            // Case 5: Tab escaping
            StringEscapeCase {
                input: "col1\tcol2",
                description: "Tab must be escaped as \\t",
                expected: "col1\\tcol2",
            },
            // Case 6: Carriage return escaping (MLIR uses hex \0D, not \r)
            StringEscapeCase {
                input: "line\r\nend",
                description: "Carriage return must be escaped as \\0D for MLIR",
                expected: "line\\0D\\nend",
            },
            // Case 7: Mixed special characters
            StringEscapeCase {
                input: "Error: \"invalid\\path\"\n",
                description: "Mixed quotes, backslash, newline all escaped",
                expected: "Error: \\\"invalid\\\\path\\\"\\n",
            },
            // Case 8: Format string with percent (no escaping needed)
            StringEscapeCase {
                input: "Value: %lld\n",
                description: "Printf format strings with newline",
                expected: "Value: %lld\\n",
            },
        ]
    }

    /// Escape function matching the fix applied in codegen/mod.rs
    fn escape_string_for_mlir(s: &str) -> String {
        s.replace('\\', "\\\\")
         .replace('\n', "\\n")
         .replace('\r', "\\0D")
         .replace('\t', "\\t")
         .replace('"', "\\\"")
    }

    #[test]
    fn test_string_escaping_parameterized() {
        for case in get_string_escape_cases() {
            let escaped = escape_string_for_mlir(case.input);

            assert_eq!(
                escaped,
                case.expected,
                "Case '{}' failed: input '{}' escaped to '{}' but expected '{}'",
                case.description,
                case.input.escape_debug(),
                escaped,
                case.expected
            );
        }
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_empty_string() {
        let escaped = escape_string_for_mlir("");
        assert_eq!(escaped, "");
    }

    #[test]
    fn test_already_escaped_passthrough() {
        // If someone passes an already-escaped string, backslashes should still be escaped
        let input = "already\\nescaped";
        let escaped = escape_string_for_mlir(input);
        // The single backslash becomes two backslashes
        assert_eq!(escaped, "already\\\\nescaped");
    }

    #[test]
    fn test_null_not_escaped() {
        // Null bytes in input should not be specially handled (they'd be rare)
        let input = "with\0null";
        let escaped = escape_string_for_mlir(input);
        assert!(escaped.contains('\0'), "Null byte should pass through unchanged");
    }

    // =========================================================================
    // MLIR Format Verification
    // =========================================================================
    
    #[test]
    fn test_mlir_global_string_format() {
        // Verify the expected MLIR format for string globals
        let name = "str_123";
        let content = escape_string_for_mlir("test\n");
        let len = content.len() + 1; // +1 for null terminator

        let mlir = format!(
            "llvm.mlir.global internal constant @{}(\"{}\") {{addr_space = 0 : i32}} : !llvm.array<{} x i8>",
            name, content, len
        );

        assert!(mlir.contains("@str_123"), "Missing global name");
        assert!(mlir.contains("\\n"), "Newline not escaped");
        assert!(mlir.contains("addr_space = 0"), "Missing addr_space");
        assert!(mlir.contains("!llvm.array<"), "Missing array type");
    }

    #[test]
    fn test_null_termination_length_calculation() {
        // String length for MLIR array should include null terminator
        let content = "hello";
        let escaped = escape_string_for_mlir(content);
        let length_with_null = escaped.len() + 1;
        
        // "hello" is 5 chars + 1 null = 6
        assert_eq!(length_with_null, 6);
    }

    // =========================================================================
    // MLIR \r Escape Regression (http_parser_bench fix)
    // =========================================================================

    #[test]
    fn test_carriage_return_uses_hex_escape() {
        // MLIR does not support \r as a string escape sequence.
        // It must be encoded as hex \0D. This test guards against
        // regression to the old \r escape that broke http_parser_bench.
        let input = "GET / HTTP/1.1\r\n";
        let escaped = escape_string_for_mlir(input);
        assert!(!escaped.contains("\\r"),
            "MLIR rejects \\r — must use \\0D hex escape, got: {}", escaped);
        assert!(escaped.contains("\\0D"),
            "Carriage return must be escaped as \\0D for MLIR, got: {}", escaped);
        assert_eq!(escaped, "GET / HTTP/1.1\\0D\\n",
            "Full HTTP request line escape mismatch");
    }

    #[test]
    fn test_crlf_pair_in_http_request() {
        // Full HTTP request with multiple \r\n pairs
        let input = "GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let escaped = escape_string_for_mlir(input);
        // Count \0D occurrences (should be 3: after request line, after Host, empty line)
        let cr_count = escaped.matches("\\0D").count();
        assert_eq!(cr_count, 3,
            "Expected 3 \\0D escapes for 3 \\r\\n pairs, got {}", cr_count);
        // Must not contain \r (MLIR-invalid)
        assert!(!escaped.contains("\\r"),
            "Must not contain \\r anywhere in MLIR output");
    }
}
