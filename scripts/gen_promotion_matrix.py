
import itertools

TYPES = ["i32", "i64", "u32", "u64", "f32", "f64"]
OPS = ["+", "-", "*", "/", "==", "!=", "<", ">"]

def get_val(ty):
    if "f" in ty:
        return "1.0"
    return "1"

def gen_code():
    code = "fn test_matrix() -> i32 {\n"
    
    # 1. Initialize variables for each type
    for t in TYPES:
        code += f"    let mut var_{t}: {t} = {get_val(t)};\n"
        
    code += "\n    # Check Promotions\n"
    
    # 2. Pairwise Operations
    count = 0
    for t1 in TYPES:
        for t2 in TYPES:
            for op in OPS:
                # We do: let res = var_t1 op var_t2;
                # We need to know the result type?
                # Codegen infers it usually.
                # Or we can just do expression statements?
                # "var_t1 + var_t2;" is valid statement in Salt if we ignore result?
                # Or assignment: "var_t1 = var_t1 + var_t2;" (only if types compatible or cast)
                # Ideally: "let tmp_xyz = var_t1 + var_t2;"
                # But we need type annotation?
                # Salt requires type annotation for let: "let x: T = ..."
                # If we don't know the promoted type, we might fail compilation if we guess wrong.
                # However, codegen `promote_numeric` logic defines the result type.
                # We can try to predict it:
                # Float > Int
                # F64 > F32
                # U64 > I64 (usually? or I64 > U32?)
                # Let's verify `codegen.rs` logic:
                # if float: f64 wins if present, else f32
                # if int: if unsigned present -> u64/u32, else i64/i32
                # size: MAX(size)
                
                # Prediction Logic
                res_ty = "i32"
                is_float = "f32" in t1 or "f32" in t2 or "f64" in t1 or "f64" in t2
                is_unsigned = "u" in t1 or "u" in t2
                is_64 = "64" in t1 or "64" in t2
                
                if op in ["==", "!=", "<", ">"]:
                    res_ty = "i32" # boolean is i32 in Salt currently
                elif is_float:
                    if "f64" in t1 or "f64" in t2:
                        res_ty = "f64"
                    else:
                        res_ty = "f32"
                else:
                    # Integer math
                    if is_64:
                        if is_unsigned: res_ty = "u64"
                        else: res_ty = "i64"
                    else:
                        if is_unsigned: res_ty = "u32"
                        else: res_ty = "i32"
                
                var_name = f"res_{t1}_{op_name(op)}_{t2}"
                code += f"    let {var_name}: {res_ty} = var_{t1} {op} var_{t2};\n"
                count += 1

    code += "\n    return 0;\n"
    code += "}\n"
    
    code += "\nfn main(argc: i32) -> i32 {\n"
    code += "    test_matrix();\n"
    code += "    return 0;\n"
    code += "}\n"
    return code

def op_name(op):
    if op == "+": return "add"
    if op == "-": return "sub"
    if op == "*": return "mul"
    if op == "/": return "div"
    if op == "==": return "eq"
    if op == "!=": return "ne"
    if op == "<": return "lt"
    if op == ">": return "gt"
    return "unknown"

if __name__ == "__main__":
    print(gen_code())
