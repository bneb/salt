
types = ["i8", "u8", "i32", "u32", "i64", "u64", "usize", "f32", "f64", "bool"]

print("fn test_matrix() {")
for t in types:
    val = "1"
    if t == "f32" or t == "f64":
        val = "1.0"
    elif t == "bool":
        val = "true"
    print(f"    let v_{t}: {t} = {val};")

print("\n    // Casts")
for i, t_from in enumerate(types):
    for j, t_to in enumerate(types):
        if t_from == t_to:
            continue
        # Only certain casts are supported by Salt's codegen logic or semantics
        # But we want to hit the codegen branches.
        # salt-front's promote_numeric has a catch-all that returns the var.
        print(f"    let _: {t_to} = v_{t_from} as {t_to};")

print("}")

print("\nfn main() -> i32 {")
print("    test_matrix();")
print("    return 0;")
print("}")
