use saltc::cli::{parse_args, run_cli, CliConfig};
use std::path::PathBuf;

#[test]
fn test_cli_usage() {
    // 1. No args -> Usage
    let args = vec!["salt-front".to_string()];
    let res = run_cli(args);
    assert!(res.is_ok()); // It OKs after printing usage
}

#[test]
fn test_cli_help() {
    // 2. Help flag
    let args = vec!["salt-front".to_string(), "--help".to_string()];
    let res = run_cli(args);
    assert!(res.is_ok());
}

#[test]
fn test_cli_file_error() {
    // 3. File not found
    let args = vec!["salt-front".to_string(), "nonexistent.salt".to_string()];
    let res = run_cli(args);
    assert!(res.is_err());
    let err = res.err().unwrap().to_string();
    assert!(err.contains("Failed to read source file"));
}

#[test]
fn test_cli_success() {
    // 4. Successful compilation
    // Use an existing simple file
    let args = vec!["salt-front".to_string(), "tests/cases/integers.salt".to_string()];
    let res = run_cli(args);
    assert!(res.is_ok());
}

#[test]
fn test_cli_release() {
    // 5. Release mode
    let args = vec![
        "salt-front".to_string(), 
        "tests/cases/integers.salt".to_string(),
        "--release".to_string()
    ];
    let res = run_cli(args);
    assert!(res.is_ok());
}

fn parse(args: &[&str]) -> anyhow::Result<Option<CliConfig>> {
    parse_args(std::iter::once("saltc").chain(args.iter().copied()).map(String::from).collect())
}

#[test]
fn test_cli_module_search_flags() {
    let config = parse(&["main.salt", "--root", "a", "--dep", "mylib=/pkg/src/lib.salt", "--root", "b"])
        .unwrap()
        .unwrap();
    assert_eq!(config.module_search.roots, [PathBuf::from("a"), PathBuf::from("b")], "--root repeats, in order");
    assert_eq!(config.module_search.deps.len(), 1);
    assert_eq!(config.module_search.deps["mylib"], PathBuf::from("/pkg/src/lib.salt"));
}

#[test]
fn test_cli_module_search_flag_errors() {
    let cases: [(&[&str], &str); 6] = [
        (&["main.salt", "--root"], "--root requires"),
        (&["main.salt", "--dep"], "--dep requires"),
        (&["main.salt", "--dep", "mylib"], "expects <name>=<file>"),
        (&["main.salt", "--dep", "=lib.salt"], "expects <name>=<file>"),
        (&["main.salt", "--dep", "mylib="], "expects <name>=<file>"),
        (&["main.salt", "--dep", "m=a.salt", "--dep", "m=b.salt"], "given twice for package 'm'"),
    ];
    for (args, expected) in cases {
        let err = match parse(args) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("{:?} should be rejected", args),
        };
        assert!(err.contains("[E004]") && err.contains(expected), "{:?}: {}", args, err);
    }
}
