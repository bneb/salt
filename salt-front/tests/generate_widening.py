
import os

types = ['u8', 'i8', 'u16', 'i16', 'u32', 'i32', 'u64', 'i64']
sizes = {'u8': 1, 'i8': 1, 'u16': 2, 'i16': 2, 'u32': 4, 'i32': 4, 'u64': 8, 'i64': 8}
signed = {'i8', 'i16', 'i32', 'i64'}

def get_larger_type(t1, t2):
    s1 = sizes[t1]
    s2 = sizes[t2]
    if s1 > s2: return t1
    if s2 > s1: return t2
    # Same size
    if t1 == t2: return t1
    # Mixed signedness same size: Salt implementation detail.
    # Based on promote_numeric viewing code: (I32, U32) returns var (reinterpretation/identity).
    # So it keeps the original type? Or does resolve logic pick one?
    # In emit_binary, commonly checks types.
    # For now, let's test that it compiles and produces correct result.
    # If we return one, assume unification picks LHS or RHS?
    # Let's assume LHS for now or verify explicitly.
    return t1 

op_tests = []
assign_tests = []
call_tests = []
main_checks = []

code = "\n"

# 1. Binary Ops
for t1 in types:
    for t2 in types:
        # Only widen small to large
        if sizes[t1] > sizes[t2]:
            target = t1
        elif sizes[t2] > sizes[t1]:
            target = t2
        else:
            continue # Skip same size for now to focus on widening

        func_name = f"test_binop_{t1}_{t2}"
        code += f"fn {func_name}(a: {t1}, b: {t2}) -> {target} {{ return a + b; }}\n"
        
        main_checks.append(f"    if {func_name}(1, 2) != 3 {{ return 100; }}")

# 2. Assignment & Return
for t_from in types:
    for t_to in types:
        if sizes[t_to] > sizes[t_from]:
            func_name = f"test_assign_{t_from}_{t_to}"
            code += f"fn {func_name}(val: {t_from}) -> {t_to} {{\n"
            code += f"    let res: {t_to} = val;\n"
            code += f"    return res;\n}}\n"
            main_checks.append(f"    if {func_name}(10) != 10 {{ return 200; }}")

# 3. Array Inference
# Test array literal inference: let x: [i64; 2] = [1, 2]; (where 1, 2 are implicitly i32?)
code += """
fn test_array_inference() -> i64 {
    let arr_i64: [i64; 2] = [10, 20];
    let arr_u64: [u64; 2] = [30, 40];
    
    # Verify values are correct (promotion worked)
    if arr_i64[0] != 10 { return 1; }
    if arr_i64[1] != 20 { return 2; }
    
    if arr_u64[0] != 30 { return 3; }
    if arr_u64[1] != 40 { return 4; }
    
    return 0;
}
"""
main_checks.append("    if test_array_inference() != 0 { return 300; }")

code += "\nfn main() -> i32 {\n"
code += "\n".join(main_checks)
code += "\n    return 0;\n}\n"

with open("tests/cases/widening_generated.salt", "w") as f:
    f.write(code)

print("Generated tests/cases/widening_generated.salt")
