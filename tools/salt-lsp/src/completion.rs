//! Salt LSP Completion — keyword + stdlib completions
//!
//! Provides context-aware completions for Salt source files.

use tower_lsp::lsp_types::*;

/// Salt language keywords
const KEYWORDS: &[(&str, &str)] = &[
    ("fn", "Function declaration"),
    ("pub", "Public visibility modifier"),
    ("let", "Variable binding"),
    ("mut", "Mutable modifier"),
    ("if", "Conditional expression"),
    ("else", "Else branch"),
    ("for", "For loop (range-based)"),
    ("while", "While loop"),
    ("loop", "Infinite loop"),
    ("match", "Pattern matching"),
    ("return", "Explicit return (required in Salt)"),
    ("break", "Break out of loop"),
    ("continue", "Continue to next loop iteration"),
    ("struct", "Struct definition"),
    ("enum", "Enum definition"),
    ("impl", "Implementation block"),
    ("trait", "Trait definition"),
    ("use", "Module import"),
    ("package", "Package declaration"),
    ("extern", "External function declaration (FFI)"),
    ("const", "Compile-time constant"),
    ("true", "Boolean true"),
    ("false", "Boolean false"),
    ("as", "Type cast"),
    ("requires", "Precondition clause (formal verification)"),
    ("ensures", "Postcondition clause (formal verification)"),
    ("concept", "Concept constraint (generic bounds)"),
    ("move", "Move ownership"),
    ("unsafe", "Unsafe block"),
];

/// Salt standard library modules
const STDLIB_MODULES: &[(&str, &str)] = &[
    ("std.core.result", "Result<T> — Ok(T) | Err(Status)"),
    ("std.core.option", "Option<T> — Some(T) | None"),
    ("std.core.ptr", "Ptr<T> — raw pointer operations"),
    ("std.core.str", "StringView — zero-copy string slicing"),
    ("std.status", "Status — canonical error codes"),
    ("std.string", "String — heap-allocated growable string"),
    ("std.io.file", "File I/O — read, write, mmap"),
    ("std.io.buffered_reader", "BufferedReader — buffered file reading"),
    ("std.fs.fs", "Filesystem — exists, create_dir, read_dir"),
    ("std.time", "Time — Instant, Duration, elapsed"),
    ("std.collections.vec", "Vec<T> — dynamic array"),
    ("std.collections.hashmap", "HashMap<K,V> — hash table"),
    ("std.net.tcp", "TCP networking"),
    ("std.net.http", "HTTP client/server"),
    ("std.thread", "Threading primitives"),
    ("std.sync", "Synchronization — Mutex, Channel"),
    ("std.json", "JSON parsing and serialization"),
    ("std.path", "Path manipulation"),
    ("std.fmt", "String formatting"),
    ("std.nn", "Neural network intrinsics"),
];

/// Salt built-in types
const BUILTIN_TYPES: &[(&str, &str)] = &[
    ("i32", "32-bit signed integer"),
    ("i64", "64-bit signed integer"),
    ("u8", "8-bit unsigned integer (byte)"),
    ("u32", "32-bit unsigned integer"),
    ("u64", "64-bit unsigned integer"),
    ("f32", "32-bit floating point"),
    ("f64", "64-bit floating point"),
    ("bool", "Boolean type"),
    ("Ptr", "Raw pointer type — Ptr<T>"),
    ("Result", "Result type — Result<T> = Ok(T) | Err(Status)"),
    ("Option", "Option type — Option<T> = Some(T) | None"),
    ("Status", "Canonical error code"),
    ("String", "Heap-allocated growable string"),
    ("StringView", "Zero-copy string slice"),
    ("Vec", "Dynamic array — Vec<T>"),
];

/// Generate completions based on the current document and cursor position.
pub fn complete(text: &str, position: Position) -> Vec<CompletionItem> {
    let lines: Vec<&str> = text.lines().collect();
    let line_idx = position.line as usize;
    if line_idx >= lines.len() {
        return all_completions();
    }

    let line = lines[line_idx];
    let col = position.character as usize;
    let prefix = if col <= line.len() { &line[..col] } else { line };
    let trimmed = prefix.trim_start();

    // After "use " → suggest stdlib modules
    if trimmed.starts_with("use ") {
        return stdlib_completions();
    }

    // After ": " or "-> " → suggest types
    if trimmed.ends_with(": ") || trimmed.ends_with("-> ") {
        return type_completions();
    }

    // Default: all completions
    all_completions()
}

/// Return info string for a keyword (used for hover).
pub fn keyword_info(word: &str) -> Option<String> {
    // Check keywords
    for (kw, desc) in KEYWORDS {
        if *kw == word {
            return Some(format!("**{}** — {}", kw, desc));
        }
    }
    // Check types
    for (ty, desc) in BUILTIN_TYPES {
        if *ty == word {
            return Some(format!("**{}** — {}", ty, desc));
        }
    }
    None
}

fn all_completions() -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (kw, detail) in KEYWORDS {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            ..Default::default()
        });
    }

    for (ty, detail) in BUILTIN_TYPES {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some(detail.to_string()),
            ..Default::default()
        });
    }

    items
}

fn stdlib_completions() -> Vec<CompletionItem> {
    STDLIB_MODULES
        .iter()
        .map(|(module, detail)| CompletionItem {
            label: module.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

fn type_completions() -> Vec<CompletionItem> {
    BUILTIN_TYPES
        .iter()
        .map(|(ty, detail)| CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_completions_not_empty() {
        let items = all_completions();
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "fn"));
        assert!(items.iter().any(|i| i.label == "return"));
    }

    #[test]
    fn test_use_triggers_stdlib() {
        let items = complete("use ", Position { line: 0, character: 4 });
        assert!(items.iter().any(|i| i.label.starts_with("std.")));
    }

    #[test]
    fn test_type_context() {
        let items = complete("let x: ", Position { line: 0, character: 7 });
        assert!(items.iter().any(|i| i.label == "i32"));
        assert!(items.iter().any(|i| i.label == "Result"));
    }

    #[test]
    fn test_keyword_info_fn() {
        let info = keyword_info("fn");
        assert!(info.is_some());
        assert!(info.unwrap().contains("Function"));
    }

    #[test]
    fn test_keyword_info_unknown() {
        assert!(keyword_info("foobar").is_none());
    }
}
