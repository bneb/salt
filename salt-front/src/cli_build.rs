use std::path::PathBuf;
use std::str::FromStr;

use crate::cli::CliConfig;

pub(crate) fn emit_sir_file(file: &crate::grammar::SaltFile, module_name: &str, output_path: Option<&str>) {
    use crate::codegen::sir::types::*;
    use crate::codegen::sir::sir_emit::*;

    let sir_module = extract_sir_from_ast(file, module_name);
    let sir_json = sir_module.to_json();
    let sir_path = output_path
        .map(|p| format!("{}.sir.json", p.trim_end_matches(".mlir")))
        .unwrap_or_else(|| format!("{}.sir.json", module_name));

    if let Err(e) = std::fs::write(&sir_path, &sir_json) {
        eprintln!("{}", crate::errors::coded("E001", format!("SIR emission failed: {}", e)));
    } else {
        eprintln!("SIR emitted: {} ({} structs, {} functions, v{})",
            sir_path, sir_module.structs.len(), sir_module.functions.len(), SIR_VERSION);
    }
}

pub(crate) fn handle_binary_synthesis(mlir: &str, basename: &str, config: &CliConfig) {
    let build_dir = std::env::temp_dir().join("salt-build");
    let mut driver = crate::driver::SaltDriver::new(build_dir);
    if let Some(ref t) = config.target_name {
        let t_parsed = crate::driver::DriverTarget::from_str(t)
            .unwrap_or_else(|e| {
                eprintln!("{}", crate::errors::coded("E010", e));
                std::process::exit(1);
            });
        driver = driver.with_target(t_parsed);
    }

    let mut output_bin = config.output_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(basename));
    // Windows executables need .exe extension
    if driver.target.exe_suffix() == ".exe" && output_bin.extension().is_none_or(|e| e != "exe") {
        output_bin.set_extension("exe");
    }

    // Compile runtime if not already present
    let rt_src = crate::driver::SaltDriver::runtime_source(&driver.target);
    if !driver.runtime_obj.exists() {
        let rt_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rt_src);
        if rt_path.exists() {
            let mut compile_cmd = std::process::Command::new(&driver.toolchain.clang);
            compile_cmd.arg("-c").arg(&rt_path).arg("-o").arg(&driver.runtime_obj);
            if driver.target.exe_suffix() == ".exe" {
                compile_cmd.arg("-target").arg("x86_64-pc-windows-msvc");
            }
            if let Err(e) = compile_cmd.status() {
                eprintln!("[E005] Failed to compile {}: {}", rt_src, e);
                std::process::exit(1);
            }
        }
    }

    eprintln!("[KeuOS] Driving MLIR -> native binary...");
    eprintln!("    Target: {:?}", driver.target);

    let is_keuos = matches!(driver.target,
        crate::driver::DriverTarget::KeuOSArm64 |
        crate::driver::DriverTarget::KeuOSX86_64
    );

    let compile_result = if is_keuos {
        eprintln!("    Linker: ld.lld (freestanding ELF)");
        driver.compile_keuos_binary(mlir, basename)
    } else {
        eprintln!("    Runtime: {:?}", driver.runtime_obj);
        driver.compile(mlir, basename)
    };

    match compile_result {
        Ok(produced_path) => {
            if produced_path != output_bin {
                if let Err(e) = std::fs::copy(&produced_path, &output_bin) {
                    eprintln!("[E005] Failed to copy binary to {:?}: {}", output_bin, e);
                    std::process::exit(1);
                }
            }

            eprintln!("[KeuOS] Running KeuOS Audit...");
            if let Ok(output) = std::process::Command::new("otool").arg("-tV").arg(&output_bin).output() {
                let disasm = String::from_utf8_lossy(&output.stdout);
                let audit_config = crate::codegen::passes::binary_audit::BinaryAuditConfig::standard(
                    crate::codegen::passes::io_backend::TargetPlatform::Darwin
                );
                let results = crate::codegen::passes::binary_audit::run_audit(&audit_config, &disasm);
                let mut all_passed = true;
                for res in results {
                    if !res.passed {
                        all_passed = false;
                        eprintln!("    Rule failed: {:?}", res.rule);
                        eprintln!("       {}", res.detail);
                    }
                }
                if all_passed {
                    eprintln!("    Audit passed.");
                } else {
                    eprintln!("    Audit found violations.");
                }
            } else {
                eprintln!("    Could not run otool to audit binary.");
            }
            eprintln!("[KeuOS] Binary synthesized: {:?}", output_bin);
            eprintln!("    Pipeline: mlir-opt -> mlir-translate -> llc (x19 reserved) -> clang (-nostdlib)");
        }
        Err(e) => {
            eprintln!("[E005] Binary synthesis failed: {}", e);
            eprintln!("    Ensure LLVM tools are installed at /opt/homebrew/opt/llvm/bin/");
            eprintln!("    Ensure keuos_rt.o is built (cd keuos_rt && make)");
            std::process::exit(1);
        }
    }
}

pub(crate) fn handle_object_synthesis(mlir: &str, basename: &str, config: &CliConfig) {
    let output_obj = config.output_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.o", basename)));

    let build_dir = std::env::temp_dir().join("salt-build");
    let mut driver = crate::driver::SaltDriver::new(build_dir)
        .with_debug_info(config.debug_info);
    if let Some(ref t) = config.target_name {
        let t_parsed = crate::driver::DriverTarget::from_str(t)
            .unwrap_or_else(|e| {
                eprintln!("{}", crate::errors::coded("E010", e));
                std::process::exit(1);
            });
        driver = driver.with_target(t_parsed);
    }

    eprintln!("[Object] Compiling to .o...");

    match driver.compile_object(mlir, basename) {
        Ok(produced_path) => {
            if produced_path != output_obj {
                if let Err(e) = std::fs::copy(&produced_path, &output_obj) {
                    eprintln!("[E006] Failed to copy object to {:?}: {}", output_obj, e);
                    std::process::exit(1);
                }
            }
            eprintln!("Object file: {:?}", output_obj);
        }
        Err(e) => {
            eprintln!("[E006] Object compilation failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Print the detailed explanation for a diagnostic code (--explain <code>).
/// The authoritative table lives in [`crate::errors::EXPLANATIONS`].
pub fn explain_error_code(code: &str) {
    println!("{}", explanation_text(code));
}

/// Render the explanation for code, or an unknown-code hint.
fn explanation_text(code: &str) -> String {
    match crate::errors::explanation(code) {
        Some(text) => text.to_string(),
        None => format!("unknown error code: {}\n\nrun saltc --explain E001 through saltc --explain E011 for known codes.", code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_codes_render_their_explanation() {
        assert!(explanation_text("E001").starts_with("[E001] File I/O Error"));
        assert!(explanation_text("E011").contains("--deny-deferred"));
    }

    #[test]
    fn unknown_code_gets_hint_with_valid_range() {
        let text = explanation_text("E999");
        assert!(text.contains("unknown error code: E999"));
        assert!(text.contains("E001") && text.contains("E011"));
    }
}
