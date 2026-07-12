import re
import sys

def sanitize_mlir(file_path):
    # Regex to find nominal struct placeholders or generic markers
    # e.g., !llvm.struct<"Vec_T", ...> or symbols like @func_T
    GENERIC_PATTERN = re.compile(r'(!llvm\.struct<"[^"]+_T"|@[^ \n]+_T[ \n\(])')
    NOMINAL_MISMATCH_PATTERN = re.compile(r'func\.call @(?P<fn>[^\s\(]+).+-> (?P<actual>[^,\n]+)')

    errors = []
    try:
        with open(file_path, 'r') as f:
            for i, line in enumerate(f, 1):
                # 1. Check for Generic Leakage
                if GENERIC_PATTERN.search(line):
                    errors.append(f"LINE {i}: Generic Leakage Detected -> {line.strip()}")
                
                # 2. Heuristic Check for Call-Site Drift (Nominal Mismatch)
                # This detects if a specialized function returns a generic placeholder
                match = NOMINAL_MISMATCH_PATTERN.search(line)
                if match and "_T" in match.group('actual'):
                    errors.append(f"LINE {i}: Nominal Drift in Return Type -> {match.group('fn')} returns generic.")
    except FileNotFoundError:
        print(f"\033[91mFAILED: File not found: {file_path}\033[0m")
        sys.exit(1)

    if errors:
        print("\033[91mFAILED: MLIR Safety Invariants Violated\033[0m")
        for err in errors:
            print(f"  {err}")
        sys.exit(1)
    
    print("\033[92mPASSED: Zero-Generic Invariant Verified\033[0m")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 sanitize_mlir.py <mlir_file>")
        sys.exit(1)
    sanitize_mlir(sys.argv[1])
