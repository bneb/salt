# Salt Concepts: Verification Constraints

## Overview

**Concepts** in Salt are compile-time verification constraints, not polymorphism mechanisms. They integrate with the Z3 theorem prover to enforce invariants.

> [!IMPORTANT]
> Concepts are NOT traits. They don't provide method dispatch or interface abstraction.

## Syntax

```salt
concept IsPositive(n: i32) requires(n > 0)

concept InBounds<T>(idx: usize, arr: &[T]) requires(idx < arr.len())
```

## How Concepts Work

1. **Declaration**: Define a constraint with a `requires` clause
2. **Compilation**: Salt emits the constraint as a boolean function
3. **Verification**: Z3 proves the constraint holds at call sites
4. **Zero Runtime Cost**: No code is generated in the final binary

## Implementation Example

A concept like:
```salt
concept IsEven(n: i32) requires(n % 2 == 0)
```

Emits this MLIR (for compile-time verification only):
```mlir
func.func private @IsEven(%arg_n: i32) -> i1 {
    %rem = arith.remsi %arg_n, %c2 : i32
    %result = arith.cmpi eq, %rem, %c0 : i32
    return %result
}
```

## Concepts vs Traits vs Generics

| Feature | Concepts | Traits | Generics |
|---------|----------|--------|----------|
| Purpose | Verification | Polymorphism | Code reuse |
| Dispatch | None | Dynamic/Static | Monomorphization |
| Runtime cost | Zero | May have vtable | Zero |
| Implementation | Z3 constraints | Not yet implemented | Type substitution |

## Use Cases

- **Bounds checking**: `concept InRange(idx, 0, len)`
- **Preconditions**: `concept NonNull(ptr)`  
- **Invariants**: `concept Sorted(arr)`
- **Type constraints**: `concept Numeric<T>` (future)

## Current Status

- ✅ Non-generic concepts work
- ✅ Z3 integration for verification
- ⏳ Generic concepts (compile-time only, no codegen)
- 🔮 Traits (polymorphism) planned for future
