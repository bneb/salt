# The Salt Programming Language — Specification

**Version 4.1 (July 2026)**  
**Status**: Draft — tracks the reference implementation (`salt-front`).

---

## Contents

1. [Lexical Structure](#1-lexical-structure)
2. [Types](#2-types)
3. [Expressions](#3-expressions)
4. [Statements](#4-statements)
5. [Functions](#5-functions)
6. [Modules and Name Resolution](#6-modules-and-name-resolution)
7. [Verification](#7-verification)
8. [Memory Model](#8-memory-model)
9. [Concurrency](#9-concurrency)
10. [Patterns](#10-patterns)
11. [The Preprocessor](#11-the-preprocessor)
12. [Standard Library](#12-standard-library)
13. [FFI and Unsafe](#13-ffi-and-unsafe)

[Appendix A: Complete EBNF Grammar](#appendix-a-complete-ebnf-grammar)  
[Appendix B: Operator Precedence](#appendix-b-operator-precedence-complete)  
[Appendix C: Compiler CLI](#appendix-c-compiler-cli)  
[Appendix D: Error Codes](#appendix-d-error-codes)

---

## Notation

Syntactic forms are specified in Extended Backus-Naur Form (EBNF, ISO/IEC 14977). A terminal symbol is written in `"double quotes"` or `'single quotes'`. A nonterminal is written in *italic*. The following meta-syntax is used:

> `[ X ]` — zero or one occurrence of X  
> `{ X }` — zero or more occurrences of X  
> `X | Y` — exactly one of X or Y  
> `( X )` — grouping

Where the grammar is insufficient to convey semantics, natural-language rules follow each production. The notation `⟦ expr ⟧` denotes the value obtained by evaluating `expr`.

This specification describes the language independently of any particular implementation. Where the reference implementation (`salt-front`) diverges, that divergence is noted.

---

## 1. Lexical Structure

### 1.1 Source Character Set

Salt source files are UTF-8 encoded. The syntax is defined over ASCII characters. Non-ASCII characters may appear only within string literals and comments.

```ebnf
SOURCE_CHAR = ? any UTF-8 code point ? ;
```

### 1.2 Comments

Line comments begin with `//` and extend to the end of the line. There are no block comments.

```ebnf
COMMENT = "//", { ? any character except newline ? }, ? newline ? ;
```

### 1.3 Whitespace

Spaces (`U+0020`), horizontal tabs (`U+0009`), carriage returns (`U+000D`), and newlines (`U+000A`) separate tokens. Whitespace carries no semantic meaning except as token delimiters.

### 1.4 Identifiers

An identifier names a variable, function, type, module, or trait.

```ebnf
IDENTIFIER = ( LETTER | "_" ), { LETTER | DIGIT | "_" } ;
LETTER     = "a".."z" | "A".."Z" ;
DIGIT      = "0".."9" ;
```

Identifiers containing `__` (two consecutive underscores) are reserved for compiler-generated names and must not be used in source code.

### 1.5 Keywords

The following are reserved and may not be used as identifiers:

```
fn   struct enum  trait  impl   let    mut    const
return  if   else  while  for    in     loop   match
break   continue  pub  unsafe self  Self  true   false
package  import requires ensures concept invariant
move  with  region  extern  as  ref  global  var
owned window  map_window
```

Additionally, `reinterpret_cast` and `shader` are reserved in specific syntactic contexts.

### 1.6 Literals

**Integer literals:**

```ebnf
INTEGER_LIT = DECIMAL_LIT | HEX_LIT | BINARY_LIT ;
DECIMAL_LIT = DIGIT, { DIGIT | "_" } ;
HEX_LIT     = "0x", HEX_DIGIT, { HEX_DIGIT | "_" } ;
BINARY_LIT  = "0b", BINARY_DIGIT, { BINARY_DIGIT | "_" } ;
HEX_DIGIT   = DIGIT | "a".."f" | "A".."F" ;
BINARY_DIGIT = "0" | "1" ;
```

An integer literal has type `i32` by default. An explicit suffix overrides: `42i64`, `255u8`, `1usize`.

**Floating-point literals:**

```ebnf
FLOAT_LIT = DECIMAL_LIT, ".", DECIMAL_LIT, [ EXPONENT ], [ FLOAT_SUFFIX ] ;
EXPONENT  = ("e" | "E"), [ "+" | "-" ], DECIMAL_LIT ;
FLOAT_SUFFIX = "f32" | "f64" ;
```

A float literal without suffix has type `f64`.

**Boolean literals:** `true` and `false`. Type: `bool`.

**Character literals:** A single character between single quotes, e.g., `'A'`. Escape sequences: `'\n'` (newline), `'\t'` (tab), `'\\'` (backslash), `'\0'` (null). A character literal has type `i8` and its value is the Unicode scalar value of the character.

**String literals:** A sequence of characters between double quotes, e.g., `"hello"`. The same escape sequences as character literals apply. A string literal has type `StringView`.

**F-string literals:** Prefixed with `f`, e.g., `f"x = {x}"`. The preprocessor expands these to `__fstring__!(...)` invocations.

**Hex literal prefix:** `hex"DEADBEEF"` expands to `__hex__!("DEADBEEF")`. Whitespace within the quotes is ignored.

### 1.7 Tokenization

The tokenizer uses maximal munch: at each position, the longest sequence of characters that matches a token production is consumed. Whitespace and comments are discarded during tokenization.

---

## 2. Types

### 2.1 Type Grammar

```ebnf
TYPE = PRIMITIVE
     | PtrType | RefType | ArrayType | TupleType
     | FnPtrType | TensorType | NamedType ;

PRIMITIVE = "i8" | "i16" | "i32" | "i64"
              | "u8" | "u16" | "u32" | "u64" | "usize"
              | "f32" | "f64" | "bool" | "char" | "()" ;

PtrType   = "Ptr", "<", TYPE, ">" ;
RefType   = "&", [ "mut" ], TYPE ;
ArrayType = "[", TYPE, ";", EXPRESSION, "]" ;
TupleType = "(", [ TYPE, { ",", TYPE } ], ")" ;
FnPtrType = "fn", "(", [ TYPE, { ",", TYPE } ], ")", [ "->", TYPE ] ;
TensorType = "Tensor", "<", TYPE, ",", "{", TENSOR_DIMS, "}", ">" ;
NamedType = IDENTIFIER, [ "<", TYPE, { ",", TYPE }, ">" ]
          | PATH, ".", IDENTIFIER, [ "<", TYPE, { ",", TYPE }, ">" ] ;
```

### 2.2 Primitive Types

| Type | Width | Signed | MLIR equivalent |
|------|-------|--------|-----------------|
| `i8` | 1 byte | Yes | `i8` |
| `i16` | 2 bytes | Yes | `i16` |
| `i32` | 4 bytes | Yes | `i32` |
| `i64` | 8 bytes | Yes | `i64` |
| `u8` | 1 byte | No | `i8` |
| `u16` | 2 bytes | No | `i16` |
| `u32` | 4 bytes | No | `i32` |
| `u64` | 8 bytes | No | `i64` |
| `usize` | platform-dependent | No | `index` |
| `f32` | 4 bytes | — | `f32` |
| `f64` | 8 bytes | — | `f64` |
| `bool` | 1 byte | — | `i1` or `i8` |
| `char` | 1 byte | — | `i8` |
| `()` | 0 bytes | — | unit |

All integer types use two's complement representation. Floating-point types follow IEEE 754. `usize` is the pointer-sized unsigned integer for the target platform.

### 2.3 Compound Types

**Pointer `Ptr<T>`**: A typed, provenance-tracked pointer to a value of type `T`. Null pointers are represented as `Ptr::empty()`. Dereferencing a `Ptr<T>` yields an lvalue of type `T`.

**Reference `&T` and `&mut T`**: A stack-rooted borrow of a value. `&T` allows read access; `&mut T` allows read-write access. References carry no ownership. The compiler verifies that references do not outlive their referent.

**Array `[T; N]`**: A fixed-size sequence of N values of type T, stored contiguously in memory. N is a compile-time constant expression. Indexing is zero-based. The `.length()` method returns N.

**Tuple `(T₁, T₂, ..., Tₙ)`**: A heterogeneous product type. The empty tuple `()` is the unit type. Tuple fields are accessed by destructuring: `let (a, b) = tuple;`.

**Function pointer `fn(T₁, ..., Tₙ) -> R`**: A first-class value representing the address of a function with the given signature. Created via `fn_addr(f)`. Called via `f(arg₁, ..., argₙ)`.

**Tensor `Tensor<T, {D₁, D₂, ..., Dₙ}>`**: A shaped multi-dimensional array. Compiles to a pointer. The `@` operator dispatches to `linalg.matmul` for two-dimensional tensors.

### 2.4 User-Defined Types

**Structs:**

```ebnf
STRUCT_DECL = [ ATTRIBUTE ], "struct", IDENTIFIER, [ "<", GENERIC_PARAMS, ">" ],
              "{", { STRUCT_FIELD }, "}" ;
STRUCT_FIELD = [ ATTRIBUTE ], IDENTIFIER, ":", TYPE, "," ;
```

A struct defines a named product type. Fields are accessed with dot notation: `s.field`.

**Enums:**

```ebnf
ENUM_DECL = "enum", IDENTIFIER, [ "<", GENERIC_PARAMS, ">" ],
            "{", { ENUM_VARIANT, "," }, "}" ;
ENUM_VARIANT = IDENTIFIER, [ "(", TYPE, { ",", TYPE }, ")" ] ;
```

An enum defines a tagged union. Each variant carries zero or more associated values. A variant with no values is a unit variant. A variant with one value carries that value directly. A variant with multiple values is stored as a tuple. Enum values are constructed as `VariantName(val₁, ..., valₙ)` and destroyed by `match`.

### 2.5 Generics

```ebnf
GENERIC_PARAMS = IDENTIFIER, { ",", IDENTIFIER } ;
```

Generic type and function parameters are monomorphized at compile time. Each unique instantiation produces a separate copy of the code. There is no runtime type erasure.

### 2.6 Memory Layout

`sizeof(T)` and `alignof(T)` are compiler-determined for each type T. The `@align(N)` attribute overrides the default alignment for a struct field. If the requested alignment cannot be satisfied (e.g., `@align(64)` on a field at offset that is not a multiple of 64), the compiler reports an error.

---

## 3. Expressions

### 3.1 Expression Classification

An expression is either an *rvalue* (produces a value) or an *lvalue* (designates a memory location). The following forms are lvalues: local variables, function parameters, field accesses `e.field`, array indexing `e[i]`, dereferences `*p`, and parenthesized lvalues. All other forms are rvalues.

### 3.2 Operator Precedence

From highest to lowest binding:

| Precedence | Operators | Associativity |
|------------|-----------|---------------|
| 17 | `.` `::` | left |
| 16 | `()` `[]` (call, index) | left |
| 15 | `-` `!` `*` `&` (unary) | right |
| 14 | `as` | left |
| 13 | `*` `/` `%` | left |
| 12 | `+` `-` | left |
| 11 | `<<` `>>` | left |
| 10 | `&` (bitwise) | left |
| 9 | `^` | left |
| 8 | `\|` | left |
| 7 | `<` `<=` `>` `>=` | left |
| 6 | `==` `!=` | left |
| 5 | `&&` | left |
| 4 | `\|\|` | left |
| 3 | `=` `+=` `-=` `*=` `/=` `%=` `<<=` `>>=` | right |
| 2 | `\|>` `\|?>` | left |
| 1 | `@` (matmul) | left |

Parentheses override precedence: `(expr)` evaluates `expr` before any enclosing operator.

### 3.3 Binary Operators

**Arithmetic:** `+`, `-`, `*`, `/`, `%` work on integer and floating-point types. Both operands must have the same type after numeric promotion. Division by zero and signed overflow are runtime errors unless the compiler proves them impossible at compile time.

**Bitwise:** `&`, `|`, `^`, `<<`, `>>` work on integer types. Shift amount must be non-negative.

**Relational:** `<`, `<=`, `>`, `>=` compare integers and floats, returning `bool`.

**Equality:** `==`, `!=` compare any two values of the same type. For structs and enums, equality is field-wise; for arrays, element-wise; for tuples, component-wise. Reference equality is NOT provided — `Ptr<T>` equality compares the addresses.

**Logical:** `&&` and `||` short-circuit. The right operand is evaluated only if the left operand does not determine the result.

### 3.4 Unary Operators

`-expr` negates an integer or float. `!expr` negates a boolean or integer (bitwise NOT for integers). `*expr` dereferences a pointer or reference, producing an lvalue. `&expr` takes the address of an lvalue, producing a reference.

### 3.5 Type Casts

`expr as T` converts `expr` to type `T`. Numeric casts between integer widths truncate or sign-extend. Float-to-integer casts truncate toward zero. The `as` operator cannot cast between unrelated pointer types; use `reinterpret_cast` in `unsafe` blocks for low-level type punning.

### 3.6 Path Expressions

A path expression refers to a named entity: a local variable, a function, a type constructor, or an imported name. Paths are dot-separated: `std.collections.HashMap`. The leading component is resolved in the current scope; subsequent components are resolved within the preceding module or type.

### 3.7 Call Expressions

`f(a₁, ..., aₙ)` calls function `f` with arguments `a₁` through `aₙ`. Arguments are evaluated left-to-right. Each argument is moved into the callee; the caller loses ownership. If the callee's parameter type is a reference, the argument is implicitly borrowed rather than moved.

### 3.8 Index Expressions

`e[i]` indexes into array `e` at position `i`. The index must be an integer type. Bounds are checked at runtime unless the compiler proves `0 <= i < e.length()` at compile time.

### 3.9 Field Access

`e.field` accesses the named field of a struct value `e`. The type of the expression is the declared type of the field.

### 3.10 Struct Literals

`TypeName { field₁: val₁, field₂: val₂ }` constructs a value of the named struct type. All fields must be provided. Field order is irrelevant. Shorthand `TypeName { x, y }` is permitted when the field name matches a variable in scope.

---

## 4. Statements

### 4.1 `let` Binding

```ebnf
LET_STMT = "let", [ "mut" ], PATTERN, [ ":", TYPE ], "=", EXPRESSION, ";" ;
```

A `let` statement introduces a new local binding. The initializer expression is evaluated and bound to the pattern. If `mut` is present, the binding is mutable. If the type annotation is omitted, the type is inferred from the initializer.

The binding is in scope from its declaration to the end of the enclosing block.

### 4.2 `let`-`else`

```ebnf
LET_ELSE_STMT = "let", PATTERN, "=", EXPRESSION, "else", BLOCK ;
```

The pattern must be irrefutable. If the expression evaluates to a value matching the pattern, the bound variables are in scope for the remainder of the enclosing block. If the expression does NOT match (e.g., `None` when `Some(x)` is expected), the `else` block is executed, which must diverge (return, break, or call a diverging function).

### 4.3 Assignment

```ebnf
ASSIGN_STMT = EXPRESSION, ASSIGN_OP, EXPRESSION, ";" ;
ASSIGN_OP   = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" ;
```

The left-hand side must be a mutable lvalue. The right-hand side is evaluated and the result is written to the location. Compound assignment operators compute the result as if by `lhs = lhs OP rhs`, except that `lhs` is evaluated once.

### 4.4 `if` Statement

```ebnf
IF_STMT = "if", EXPRESSION, BLOCK, [ "else", ( BLOCK | IF_STMT ) ] ;
```

The condition expression must have type `bool`. If true, the first block is executed; otherwise, the `else` block (if present) is executed. `if` is an expression when every branch produces a value of the same type.

### 4.5 `while` Statement

```ebnf
WHILE_STMT = "while", EXPRESSION, BLOCK ;
```

The condition is evaluated before each iteration. If true, the block is executed and the loop repeats. If false, execution continues after the block.

### 4.6 `for` Statement

```ebnf
FOR_STMT = "for", IDENTIFIER, "in", EXPRESSION, BLOCK ;
```

The expression must produce an iterator (a value with a `.next()` method). The loop variable takes each successive element. The loop variable is immutable.

### 4.7 `loop` Statement

```ebnf
LOOP_STMT = "loop", BLOCK ;
```

Infinite loop. Exit via `break` or `return`.

### 4.8 `match` Statement

```ebnf
MATCH_STMT = "match", EXPRESSION, "{", { MATCH_ARM, "," }, "}" ;
MATCH_ARM  = PATTERN, [ "if", EXPRESSION ], "=>", ( BLOCK | EXPRESSION, "," ) ;
```

The scrutinee is evaluated. Arms are tested in order. The first arm whose pattern matches (and whose optional guard evaluates to `true`) is executed. The compiler verifies exhaustiveness: every possible value of the scrutinee type must be covered by at least one arm.

### 4.9 `return` Statement

```ebnf
RETURN_STMT = "return", [ EXPRESSION ], ";" ;
```

Evaluates the expression (if present) and transfers control to the function's caller. The expression type must match the function's declared return type. In a function returning `()`, `return;` with no expression is permitted.

### 4.10 `unsafe` Block

```ebnf
UNSAFE_BLOCK = "unsafe", BLOCK ;
```

Within an `unsafe` block, raw pointer arithmetic, `reinterpret_cast`, and direct memory operations are permitted. The compiler does not verify memory safety within `unsafe` blocks.

### 4.11 `region` Block

```ebnf
REGION_STMT = "with", "region", IDENTIFIER, BLOCK ;
REGION_CALL = "region", "(", STRING_LIT, ")", BLOCK ;
```

Declares a memory region. All arena allocations within the block are associated with the named region. When the block exits, the region is freed.

---

## 5. Functions

### 5.1 Function Declarations

```ebnf
FN_DECL = { ATTRIBUTE }, [ "pub" ], "fn", IDENTIFIER,
          [ "<", GENERIC_PARAMS, ">" ],
          "(", [ PARAMS ], ")", [ "->", TYPE ],
          { CONTRACT_CLAUSE },
          BLOCK ;
PARAMS = PARAM, { ",", PARAM } ;
PARAM  = PATTERN, ":", TYPE ;
```

A function declares a named, callable computation. `pub` makes it visible outside the current module. Generic parameters enable parametric polymorphism. The return type defaults to `()` if omitted.

### 5.2 Attributes

Attributes are prefixed with `@` and appear before the item they modify.

| Attribute | Valid on | Effect |
|-----------|----------|--------|
| `@inline` | Functions | Hint to inline at call sites |
| `@trusted` | Functions | Bypass contract verification for the function body. Used for FFI wrappers and hand-audited code. |
| `@export` | Functions | Emit with C-compatible symbol name (no name mangling) |
| `@yielding(N)` | Functions | Inject a yield point every N loop iterations, enabling cooperative scheduling. If N is omitted, a default interval is used. |
| `@pulse(N)` | Functions | Register the function to be invoked at N Hz by the scheduler's pulse timer. The function must take no arguments and return nothing. |
| `@align(N)` | Struct fields | Override the field's alignment to N bytes. The compiler verifies the requested alignment is satisfiable. |
| `@derive(T₁, ..., Tₙ)` | Structs, enums | Auto-generate trait implementations from the type's fields or variants.

### 5.3 Move Semantics

When a function is called, each argument is *moved* into the corresponding parameter. The caller's binding becomes unavailable. Subsequent use of a moved variable is a compile-time error.

If the parameter type is a reference (`&T` or `&mut T`), the argument is *borrowed* rather than moved. The caller retains ownership for the duration of the call. References must not outlive their referent; the compiler verifies this intraprocedurally via the scope ladder.

### 5.4 Function Pointers

A function pointer type `fn(T₁, ..., Tₙ) -> R` designates the address of any function with that signature. The built-in `fn_addr(f)` returns a function pointer to `f`. Calling a function pointer has the same syntax and semantics as calling a named function.

### 5.5 Extern Functions

```ebnf
EXTERN_FN_DECL = "extern", "fn", IDENTIFIER, "(", [ PARAMS ], ")", [ "->", TYPE ],
            { CONTRACT_CLAUSE }, ";" ;
```

Declares a function with C ABI linkage. Only primitive types, `Ptr<T>`, and function pointers may appear in the signature of an `extern fn`. The body is provided by an external object file.

---

## 6. Modules and Name Resolution

### 6.1 Package Declaration

```ebnf
PACKAGE_DECL = "package", IDENTIFIER, { ".", IDENTIFIER }, ";" ;
```

A package declaration names the module. It must appear as the first non-comment item in a source file.

### 6.2 Imports

```ebnf
IMPORT_DECL = "import", PATH, [ ".", ( "*" | "{", IDENTIFIER, { ",", IDENTIFIER }, "}" ) ], ";" ;
PATH        = IDENTIFIER, { ".", IDENTIFIER } ;
```

An import makes names from another module available in the current scope. `import std.core.ptr.*` imports all public names from `std.core.ptr`. `import std.io.file.{File, BufferedReader}` imports specific names.

### 6.3 Visibility

A top-level item prefixed with `pub` is visible to other modules. Without `pub`, the item is private to the declaring module. Struct fields follow the struct's visibility; there is no per-field visibility.

---

## 7. Verification

### 7.1 Contracts

```ebnf
CONTRACT_CLAUSE = ( "requires" | "ensures" ), EXPRESSION, ";" ;
```

A `requires` clause declares a precondition: a boolean expression that must hold at every call site. A `ensures` clause declares a postcondition: a boolean expression that must hold at every return site. The special identifier `result` in an `ensures` clause refers to the function's return value.

### 7.2 Verification Model

Contracts are checked at compile time using an SMT solver. The process for each contract clause is:

1. The compiler substitutes the actual arguments into the expression.
2. The constant folder attempts to reduce the expression. If it evaluates to `true`, the check is elided — zero runtime instructions are emitted.
3. If constant folding fails, the compiler checks whether the negation of the expression is satisfiable.
4. **Proved:** No input can violate the condition. The check is elided.
5. **Counterexample:** A violating input exists. The compiler reports the specific values and stops with an error.
6. **Timeout:** The solver cannot decide within a fixed time budget (100ms). The compiler emits a runtime assertion as a fallback. The program compiles and runs, but will trap if the condition is violated at runtime.

Postconditions are checked similarly at each return site, with `result` bound to the returned expression.

### 7.3 Type-Bound Proofs

Before checking a contract, the compiler injects type range constraints into the solver. For a parameter `x: u8`, the solver is informed that `0 <= x <= 255`. This means `requires(x < 256)` on a `u8` parameter is trivially proved for any argument value, even when the argument is not a compile-time constant.

### 7.4 Loop Invariants

While loops may contain `invariant` statements that the compiler checks using Hoare logic:

```salt
let mut i: i64 = 0;
while i < 5 {
    invariant i >= 0 && i < 5;
    arr[i] = 0;  // Z3 proves this is safe using the invariant
    i = i + 1;
}
```

The compiler verifies two properties:
1. **Base case**: the invariant holds at loop entry
2. **Inductive step**: assuming the invariant and loop condition hold, the invariant still holds after one iteration (modelled by havocking modified variables)

If either check fails, the compiler reports a counterexample. Invariants that constrain an index variable to a known range enable Z3 to prove array bounds safety inside while loops — the same way `for`-loop induction variables do automatically.

### 7.5 Limitations

Contracts cannot prove all properties. Known limitations of the current implementation:

- Integer overflow/wraparound is not modeled. Arithmetic is unbounded
  (arbitrary-precision) internally, not the fixed-width, wrapping arithmetic
  real hardware performs. `ensures { (a + 1) > a }` on a `u64` proves, even
  though it is false at `a = u64::MAX` on real hardware. Practically: an
  overflow GUARD written in your own code (`if a + b < a { reject }`) still
  executes as real, correctly-wrapping machine code at runtime and remains
  fully protective -- what does NOT hold is any expectation that the prover
  will catch you if you forget to write one. Removing such a guard and
  re-proving is not, by itself, evidence the guard was unnecessary.
- Floating-point properties: the solver's theory of floating-point arithmetic is incomplete. Contracts with non-trivial float expressions may timeout.
- String length and content: only compile-time-known string literals are reliably folded to constants. Properties of strings from runtime sources (I/O, network) rely on the timeout fallback.
- Non-linear integer arithmetic: multiplication of two variables may timeout.
- Fields of a returned value: `ensures { result.field < N }` is not proven, because
  the postcondition is checked against a single symbolic `result` rather than
  against the struct's fields. Constrain the field through a scalar the function
  already returns, or check it at runtime.
- ~~A callee's `ensures` is not assumed at the call site.~~ **Fixed for named
  results.** After `let y = f(x);`, `f`'s `ensures` clause (params
  substituted with the actual arguments, `result` tied to `y`) is now a fact
  available to any later `requires`/`ensures` check in `y`'s scope. Two prior
  bugs made this a no-op despite the plumbing (`apply_ensures_to_solver`,
  called from every call site) looking complete: `result` lowered to a
  symbol literally named `"result"` -- the same name at every call site with
  an ensures clause, colliding across all of them -- with no substitution
  rule ever pointing it at the value this specific call produced; and the
  resulting fact was asserted only into `ctx.z3_solver`, which
  `requires`/`ensures` checks never read (each builds a fresh `Solver`; only
  while-loop invariant proving reads it directly). `test_cross_fn_chain.salt`
  -- already in the suite, already claimed as a working example of this --
  measurably improved from this fix (1/2 checks proven to 2/2) once it was
  actually true rather than aspirational. Still doesn't reach an UNNAMED call
  result (`h(g(x))` -- `g`'s value is never bound to an identifier, so
  nothing carries the fact forward) or a recursive call's own inductive
  hypothesis (no ranking/induction machinery exists for that).
- ~~A function's own `requires` clause is not assumed as a fact for
  reasoning inside its own body.~~ **Fixed for calls it makes.** `f(x)
  requires { x <= LIMIT } { g(x); }`, where `g` also requires `x <= LIMIT`,
  now proves without re-guarding. The mechanism (`caller_preconditions`,
  pushed/popped around the function body, consumed at the callee's
  requires-check site) already existed and was already wired correctly --
  `requires { expr }` just parses as `Expr::Block`, and the Z3 translator
  has no arm for that, so every entry silently failed to translate and
  never reached the solver. `requires(expr)` (paren form) was never
  affected, which is why this stayed hidden. This specifically covers calls
  the function makes; a path-sensitive `if` guard, re-stating the same fact
  directly in the body, is still the way to help checks that aren't a call
  (e.g. an inline array index) discharge against a `requires` clause.
- ~~Unsigned integer types carry no non-negativity constraint.~~ **Fixed.**
  `u8/u16/u32/u64/usize` (and the signed narrow types `i8/i16`) now carry
  their type's range as an implicit fact wherever a value of that type is in
  scope for a `requires` or `ensures` check -- not only for the direct
  arguments of the call being verified, which is as far as the existing
  bounds-injection reached before. So `dense_idx < count` now DOES imply
  `count > 0` for unsigned types, without an extra guard stating it.
  Scoped to the free variables the constraint under check actually mentions,
  not every local in scope: the unscoped version measurably regressed one
  proof-gate fixture (a bitvector-heavy check pushed past its 100ms
  watchdog by the extra assertions) before this constraint was added.
- A `Ptr<T>.field` read is opaque in ARGUMENT position, not only when
  returned: two occurrences of the same `set[0].count` are two unrelated
  symbols to the solver, even with nothing mutated between them. Bind the
  read to a local once and use the local everywhere the fact is needed.
- ~~Locals are not constrained to their defining expressions.~~ **Fixed for
  non-`mut` locals.** After `let end = ptr + len;`, `end` and `ptr + len` are
  now the same fact to the solver: a guard on one now discharges an
  obligation stated over the other, in either direction. Sound
  unconditionally for a non-`mut` binding -- it has exactly one value for
  its whole lifetime, so there is no branch to merge and no havoc semantics
  to fight. Implemented as a new `ctx.emission.let_bindings` fact list
  (`name == init`, one entry per non-`mut` local in scope), asserted
  alongside `path_conditions` at both the `requires` and `ensures` check
  sites; kept as its own list rather than pushed onto `path_conditions`
  itself because that Vec's push/pop pairs assume strict LIFO nesting by the
  if/else branch that owns each entry, and a let's scope (to the end of its
  enclosing block) doesn't nest that way. `emit_block` and `emit_block_expr`
  each snapshot-and-truncate it, so a let declared inside a branch or loop
  body doesn't leak its fact past that block. See
  `tests/z3_contracts/test_let_binding_proved.salt` and
  `test_let_binding_rejected.salt`. Still open: a `mut` local's own
  defining expression is unconstrained (stays on the literal-only
  `assert_local_lit_int_in_z3` path below it) -- extending this to `mut`
  locals hits the havoc entry two bullets down, not this one.
- A `mut` local reassigned anywhere -- not only inside a branch -- loses its
  tracked value for the solver. This is deliberate: the same "havoc" semantics
  that make while-loop verification sound (a loop's induction variable must be
  treated as unconstrained going into each iteration, or reasoning about the
  loop would be unsound) apply to any reassignment, named literally
  `{var}_havoc_{id}` at the one site that mints them
  (`codegen/stmt/while_stmt.rs`). `ensures` clauses handle this gracefully: a
  postcondition whose return value traces to a havoc'd local defers to a
  runtime check rather than hard-failing (`emit_ensures_runtime_check`).
  `requires` clauses at a call site do not -- a havoc'd argument produces a
  hard `E009`, unconditionally, even when the surrounding code is correct
  (this is what forces the pattern seen throughout this codebase of
  re-asserting a bound immediately before the call it protects, rather than
  trusting an earlier clamp).

  This asymmetry looks fixable by mirroring the `ensures` treatment -- detect
  a havoc'd argument, defer to `emit_requires_runtime_check` instead of
  failing -- and a patch doing exactly that was built, and reverted, in the
  course of documenting this entry. It passed every existing contract test
  except one: `test_slice_cursor_rejected`, built specifically to catch
  "unsound elision" in bounds checks, where a genuinely wrong loop bound
  (`while off <= buf.len()`, allowing `off == len` into `buf.set()`) also
  havocs its induction variable and was accepted by the same broadened
  leniency. From the solver's side, "a benign clamp the prover cannot see
  through" and "a genuinely wrong loop bound" are the identical shape --
  both are SAT results driven by a havoc'd symbol -- and nothing in scope at
  the call site distinguishes them. Fixing this soundly needs a way to tell
  those two cases apart that does not currently exist, not a bigger
  allowlist; recorded here rather than shipped narrower-than-safe. **Still
  open** -- the paragraph below fixes an adjacent, different gap found
  while re-investigating this one, not this one.

  ~~Nothing re-established anything about a havoc'd variable once its loop
  exited, not even the loop's own invariant.~~ **Fixed.** A `while` loop's
  standard Hoare post-loop fact -- `invariant && !cond` -- is now pushed
  onto `ctx.emission.scoped_facts` when the loop exits (renamed from
  Tier 1's `let_bindings`; the field now serves both), so code AFTER a
  loop can use it, the same way `loop_assumptions` already lets code
  INSIDE the loop use `invariant && cond`. Before this fix, `off` stayed
  permanently, unconditionally free for the rest of the function the
  moment any while loop havoc'd it -- not just within the loop, which is
  what the paragraph above is about, but forever after, since nothing
  ever un-havocs `symbolic_tracker`'s entry for a name. Soundness rests on
  `__salt_contract_violation` being `noreturn` (see context.rs's
  cold+noreturn passthrough): invariant maintenance across iterations is
  checked at RUNTIME, not proven at compile time (only the base case is),
  so this is sound because control can only reach post-loop code if that
  runtime check held on every iteration that ran -- the same trust
  `emit_requires_runtime_check`/`emit_ensures_runtime_check` already rest
  on elsewhere, applied somewhere it wasn't reaching before, not a new
  kind of trust.

  A second, genuine soundness bug was found and fixed in the same change:
  `scoped_facts` entries are raw expressions re-resolved by NAME at
  whatever point they're later consulted, and a loop's induction variable
  is an ordinary, reusable name. Two SEQUENTIAL while loops reusing the
  same variable name produced a direct contradiction -- the first loop's
  stale post-fact and the second loop's live one, both resolving against
  the second loop's havoc symbol once the name was reassigned -- which
  silently made the solver context UNSAT and let an absurd, unrelated
  claim "prove" for the rest of the block. Caught empirically
  (`test_while_post_loop_var_reuse_rejected.salt` reproduces it) while
  testing this fix, not by inspection. Fixed by anchoring each pushed
  fact to its own loop's permanent `{var}_havoc_{id}` symbol
  (`HavocAnchor` in `while_stmt.rs`) instead of the bare, reusable name --
  the same technique Tier 2 used for call results, applied here because
  the same class of risk turned out to apply to loop variables too.

  For-loops were not checked for the same gap -- `for_loop.rs` pops
  `loop_assumptions` the same way but has no post-loop-fact push at all,
  and for-loop induction variables typically go out of scope entirely at
  the loop (unlike a `while` loop's condition variable, a pre-declared
  `mut` local that outlives it), so whether there's anything left to
  propagate is unclear without a closer look.

  The still-open within-loop problem two paragraphs up turned out to be
  smaller than it looked: tested directly, `need_positive(y)` inside a
  loop that mutates `y` fails with no invariant covering it, and
  compiles clean the moment `invariant y > 0;` is added -- no leniency,
  no runtime fallback, 100% proven. So this was never "impossible to
  verify without real invariant inference"; it's "the compiler doesn't
  tell you which invariant to add," and a bare `y_havoc_14` in the
  counterexample doesn't point at `invariant` the way a plain English
  hint could. `test_slice_cursor_rejected` stays correctly rejected
  either way -- no invariant rescues a genuinely wrong guard. Fixed with
  a new `ProofHint::AddInvariant` (`proof_witness.rs`), triggered by a
  `_havoc_` substring in the failing constraint and replacing the
  generic `AddRequires`/`AddAssert` hints entirely rather than
  supplementing them (those reference a `requires` clause on the
  function signature or a bare `assert` -- Salt has no `assert`
  statement at all, and a `requires` clause can't refer to a local
  variable's value; both would send the developer at the wrong fix for
  a loop-local). The suggested condition is rewritten into the caller's
  own terms (the callee's parameter names replaced with the actual
  argument expressions at this call site) rather than shown verbatim in
  the callee's names, which would reference identifiers out of scope at
  the call site whenever the argument isn't a bare variable matching
  the parameter's own name. Diagnostics only -- doesn't change what's
  provable, so it carries none of the soundness risk the reverted
  leniency patch had.

  ~~Automatically trying a failing call's `requires` clause as a
  candidate invariant would remove the manual step entirely; not
  attempted, deliberately, to see how much friction the hint alone
  removes first.~~ **Built anyway, same session, on request rather than
  waiting for real usage to answer that question.** For every
  statement-position call in a while loop's body (`need_positive(y);` --
  not `let x = f(y);`, not a method call like `buf.set(off, v)`; both
  need more than a name-based lookup and are out of scope for now),
  `collect_call_requires_candidates` (`while_stmt.rs`) proposes the
  callee's own `requires` clause, substituted into the loop's variable
  names, as a candidate invariant; `filter_call_requires_candidates`
  keeps only the ones that hold at the base case, silently dropping the
  rest. A dropped candidate changes nothing -- the call it came from
  still gets checked for real, with concrete arguments, during body
  emission, by the same path that already existed. A kept candidate is
  inserted as a genuine `Stmt::Invariant`, which means it gets the exact
  same treatment a hand-written one does: included in `loop_assumptions`
  for other calls inside the loop, in `scoped_facts` after it (the
  paragraph above), and -- not incidental, load-bearing -- a real
  per-iteration runtime check via the normal `Stmt::Invariant` codegen.
  Without that last part, a wrongly-kept candidate would have no safety
  net at all, unlike an explicit invariant, which at least gets caught
  at runtime if it's wrong; this mechanism inherits the exact same
  guarantee rather than a weaker one, by reusing the exact same
  insertion point `try_infer_while_invariant`'s existing single-pattern
  auto-invariant already uses.

  Building this immediately surfaced a real, separate, pre-existing bug
  it doesn't cause but reliably triggers: `symbolic_tracker` maps a
  variable's SOURCE NAME (not a unique id) to its current Z3 term, and
  is never reset between functions. Two functions in the same file each
  using "y" as a loop variable meant the second function's base-case
  check -- for a HAND-WRITTEN invariant, not just a synthesized
  candidate -- silently resolved against the first function's leftover
  havoc symbol and failed for a reason that had nothing to do with
  either function's own code. Reproduces with two copies of the exact
  same, individually-correct function body compiled together
  (`test_cross_fn_symbolic_tracker_proved.salt`). Fixed by clearing
  `symbolic_tracker` at the start of `emit_fn` (`codegen/mod.rs`),
  alongside the existing per-function clears of `consumed_vars`/
  `consumption_locs`/`devoured_vars`. `ctx.z3_solver`'s own accumulated
  assertions are left alone -- they're keyed by these same never-reused
  names, so once nothing can look them up by name anymore they go inert
  rather than harmful, and resetting the solver itself is a larger,
  less-understood change than clearing this one cache.

  `symbolic_tracker` living on `CodegenContext` -- constructed once per
  compilation, not once per function -- rather than being reset per call
  is one instance of a general shape: any `RefCell` field there keyed by
  plain, reusable source-level name is a candidate for the same class of
  bug. Prompted an audit of the others. `ownership_tracker`
  (`Z3StateTracker`), `malloc_tracker`, and `arena_escape_tracker` turned
  out to already be correctly scoped, just via a different mechanism --
  `emit_fn` swaps each for a fresh instance and restores the caller's on
  the way out (`ctx.malloc_tracker.replace(MallocTracker::new())`, etc.,
  `codegen/mod.rs`), rather than clearing in place. `pointer_tracker` was
  the one actual miss: `PointerStateTracker::states` is keyed by plain
  variable name exactly like `symbolic_tracker`, ships its own `clear()`
  explicitly documented "for new function scope"
  (`verification/pointer_state.rs`), and was simply never wired up.
  Confirmed with an adversarial two-function case
  (`test_cross_fn_pointer_tracker_rejected.salt`): a bare-alias local
  (`let p = other;`) never re-marks its own tracker entry (see
  `emit_local_pointer_tracking`, `stmt/mod.rs` -- it only handles a
  recognized constructor call or no initializer at all), so it silently
  inherited an unrelated earlier function's leftover `Valid` marking for
  the name `p`, and `requires { valid(p) }` on a call using it was PROVEN
  -- "0 deferred to runtime" -- for a pointer this function never
  actually validated. Unlike the `symbolic_tracker` fix, this one is
  wired in as a swap-and-restore, matching its three siblings above, not
  a bare clear: `process_fn_arguments`, which marks pointer-typed
  parameters `Valid` on entry, runs before the point where those three
  swap in fresh state, so the swap-in for `pointer_tracker` sits earlier
  in `emit_fn`, right next to `symbolic_tracker`'s clear -- clearing
  after parameter processing would have wiped out this function's own
  parameter marks. The restore stays grouped with the other three near
  the end of `emit_fn`; that ordering isn't sensitive the way swap-in is,
  since nothing checks `pointer_tracker` as a whole the way
  `malloc_tracker.verify()` does. Swap-and-restore over a bare clear
  everywhere it was available (not just for consistency): if `emit_fn`
  is ever reentered mid-body for nested specialization, clearing would
  permanently discard the outer call's in-progress state, where
  swap-and-restore recovers it.

  Two adjacent, separate findings surfaced while constructing the
  adversarial test above; both since resolved. (1) `process_fn_arguments`
  marks every pointer-typed parameter `Valid` on function entry
  unconditionally, not only when the function's own `requires` actually
  says so about it. Investigated as a possible bug matching
  `pointer_state.rs`'s own doc comment ("Optional... function args"), but
  it's the load-bearing ergonomic default, not an oversight: `check_deref`
  treats untracked (`None`) identically to `Valid`, so an ordinary
  function that dereferences its own pointer parameter with no `requires`
  at all -- the common case -- compiles clean today
  (`buf.write(val)` with no contract on `buf`), and `requires { valid(p) }`
  doesn't mark `p` in `pointer_tracker` either (`emit_requires_verification`
  only asserts into the Z3 solver), so there's no opt-in path to a
  stricter default even for a function that states one. Defaulting
  raw-pointer parameters to `Optional` instead would break that common
  case outright with no fallback -- a design change requiring its own
  buy-in, not a fix, and out of scope here. (2) Any call to an extern (or
  configured freeing) function marks its own pointer arguments `Optional`
  as a post-emission side effect (`emit_low_level_call`, `call_helpers.rs`)
  -- correct for code that runs *after* that call. Checked directly
  whether a single call site's own `requires { valid(...) }` could be
  validated against tracker state read *after* this side effect: it
  isn't -- verification happens before `emit_low_level_call` runs for that
  same call, confirmed by tracing `pointer_tracker.get_state` at the
  point of injection, and a plain user-function call in between two
  checks doesn't trigger the downgrade at all (only extern/freeing calls
  do). No bug.

  A third, unrelated, considerably more serious bug turned up by accident
  while building an adversarial case for (1)/(2) above: a dereference
  safety check that was already computing the right answer was being
  silently discarded before it could ever reject anything.
  `try_emit_special_method` (`special_methods.rs`) calls `check_deref` for
  unsafe `Ptr<T>` methods (`.read()`/`.write()`/`.offset()`/etc.) and
  correctly returns `Err` on a real violation -- Freed, Uninitialized,
  Empty, or Optional -- distinct from `Ok(None)` when the method name
  isn't a special method at all. Its caller, `emit_method_call`
  (`calls.rs`), matched `if let Ok(Some(res)) = try_emit_special_method(...)`,
  which treats those two outcomes identically: a genuine `Err` silently
  fell through to ordinary, unchecked method resolution instead of
  aborting. `free(p); p.read();` -- one function, no contracts, no
  cross-function state whatsoever -- compiled clean with zero error and
  zero runtime check. Confirmed by tracing `check_deref`'s own return
  value directly at the call site: it correctly computed `Freed`, and the
  compiler emitted the read anyway. Fixed by propagating with `?` instead
  of matching on `Ok(Some(_))`, so `Ok(None)` still falls through but
  `Err` now aborts with the real message. See
  `test_use_after_free_via_read_rejected.salt`. The identical shape
  appeared twice more, both wrapping `ctx.emit_intrinsic(...)` (`calls.rs`,
  `method_resolution.rs`) -- lower severity (a real intrinsic-argument
  error there gets a confusing generic-method-resolution error instead of
  its own specific one, rather than silently compiling), but same fix,
  applied for the same reason. All three fixes are a five-line diff
  total; the full test suite (2025 Rust tests, 69 z3_contracts fixtures)
  passes unchanged before and after, meaning nothing was relying on the
  swallowed behavior.
- Conditionally-assigned `mut` locals lose their constraints. After
  `let mut x = a; if c { x = b; }` the solver does not merge the branches, so a
  guard on `x` will not discharge a later obligation about it. Where this
  matters, state the precondition immediately before the operation it protects.
- The `@trusted` attribute bypasses verification entirely for the annotated function body.

Postconditions on bool-returning functions used to be skipped in silence, which
is the more dangerous shape of the same problem: the return value was translated
through the integer path, which had no arm for bool literals or for comparisons
in value position, so translation failed and no check ran. `ensures { result }`
on `return false` compiled clean and reported nothing. Bools now carry as 0/1 in
that encoding and a bare identifier in boolean position means "not zero"; see
`tests/z3_contracts/test_bool_postcondition_proved.salt`.

Named constants used to belong on this list by accident: a `const` referenced in
a contract lowered to a fresh unconstrained symbol rather than its value, so
`ensures { result < MAX }` became `result < <anything>` and Z3 duly produced a
counter-example. The identical contract written with a literal proved fine,
which made the failure look like a limit of the solver. Constants now lower to
their values; see `tests/z3_contracts/test_const_in_contract_proved.salt`.

A postcondition's type-bounds scoping used to look only at identifiers written
in the `ensures` clause's own text, not at the RETURN expression `result` is
bound to. `fn g(n: u64) -> u64 ensures { result > 0 } { return n + 1; }` --
obviously true for real `u64` arithmetic -- failed to prove, because the
scoping never saw `n` (it appears only inside `result == n + 1`, the
Hoare/WP binding, not inside `result > 0` itself), so `n`'s non-negativity
was never available and Z3 was free to pick `n = -1`. Found while testing
the composability fixes above and confirmed to predate both of them.
Type-bounds scoping now also collects from the return expression; see
`tests/z3_contracts/test_ensures_return_expr_bounds_proved.salt`.

---

## 8. Memory Model

### 8.1 Allocation

Salt provides three allocation strategies:

**Arena allocation:** `Arena::new(capacity)` creates a bump-allocated region. `arena.alloc::<T>()` allocates a value of type T within the region. `arena.alloc_bytes(n)` allocates n bytes. `arena.mark()` captures the current offset. `arena.reset_to(mark)` frees all allocations since the mark. Arena allocation is O(1) and deterministic; the region is bulk-freed.

**Heap allocation:** `HeapAllocator` wraps the platform `malloc`/`free`. Used via `Box::new(value)` or `Vec::with_capacity(n)` with a heap allocator.

**Prelude default:** The default allocator is an arena. `Vec::new()` and `String::new()` use the prelude's `DefaultAllocator` unless an explicit allocator is provided.

### 8.2 Moves

Assignment, function argument passing, and return transfer ownership. After `let y = x;`, `x` is uninitialized and cannot be used. To explicitly transfer ownership, write `move x;`.

### 8.3 Scope Ladder

The compiler tracks the *depth* of every pointer expression:

- Depth 0: globals and statics. Live for the program's duration.
- Depth 1: function arguments. Live for the call's duration.
- Depth 2+: local variables. Live for the enclosing block's duration. Nested blocks increase depth.

Three rules govern pointer safety:

1. **Return Rule:** A pointer of depth d >= 2 must not be returned. The return value must have depth <= 1.
2. **Assignment Rule:** A pointer `b` can be stored into location `a` only if depth(b) <= depth(a). Shorter-lived pointers cannot be stored into longer-lived containers.
3. **Transitivity:** Field access and indexing preserve the depth of the parent object.

Violations are reported at compile time with reference to the specific rule violated.

---

## 9. Concurrency

Salt supports cooperative concurrency through two function attributes.

`@yielding(N)` annotates a function as cooperatively yielding. The compiler injects a yield point every N loop iterations, allowing the scheduler to preempt the function between iterations. If N is omitted, a default interval is used. The function must not hold locks across yield points.

`@pulse(N)` registers a function to be invoked by the scheduler's pulse timer at N Hz. The function must take no arguments and return nothing. Pulse functions are used for periodic tasks like cursor blinking, keepalive packets, and I/O polling.

These attributes target KeuOS's scheduler. In native (non-KeuOS) builds, `@yielding` has no effect and `@pulse` functions are never called.

---

## 10. Patterns

```ebnf
PATTERN = "_"                                  (* wildcard *)
        | LITERAL                              (* literal match *)
        | [ "mut" ], IDENTIFIER                (* binding *)
        | PATH, "(", [ PATTERN, { ",", PATTERN } ], ")"  (* enum variant *)
        | "(", [ PATTERN, { ",", PATTERN } ], ")"        (* tuple *)
        | PATH, "{", [ FIELD_PAT, { ",", FIELD_PAT } ], "}"  (* struct *)
        | PATTERN, "|", PATTERN                (* or-pattern *)
        ;
FIELD_PAT = IDENTIFIER, [ ":", PATTERN ] ;
```

A pattern is *irrefutable* if it matches every value of the scrutinee type. A pattern is *refutable* otherwise. `let` and function parameters require irrefutable patterns. `match` arms may be refutable.

---

## 11. The Preprocessor

Before parsing, the Salt source text undergoes the following transformations:

1. `use a.b.c` is rewritten to `import a.b.c`.
2. `a::b::c` path syntax is rewritten to `a.b.c`.
3. `f<A, B>(x)` is rewritten to `f::<A, B>(x)` (turbofish insertion).
4. `a |> f(b, _)` is rewritten to `f(b, a)` (pipe operator).
5. `a |?> f(b, _)` is rewritten to `__railway__!(a, f(b, _))`.
6. `a @ b` is rewritten to `a.matmul(b)`.
7. `f"text {expr}"` is rewritten to `__fstring__!("text {expr}")`.
8. `target.f"text {expr}"` is rewritten to `__target_fstring__!(target, "text {expr}")`.
9. `hex"AB CD"` is rewritten to `__hex__!("ABCD")`.
10. `expr~` is rewritten to `__force_unwrap__!(expr)`.
11. `@derive(T₁, T₂)` on a type declaration is expanded to the corresponding `impl` blocks.

These transformations are purely syntactic and do not affect the semantics of the resulting program.

---

## 12. Standard Library

The standard library is organized under `std.`. The module tree is:

| Path | Contents |
|------|----------|
| `std.core.ptr` | `Ptr<T>`, `Ptr::empty()`, pointer utilities |
| `std.core.option` | `Option<T>` (Some/None) |
| `std.core.result` | `Result<T>` (Ok/Err with `Status`) |
| `std.core.str` | `StringView`, string operations |
| `std.core.iter` | `Range`, iterator combinators |
| `std.core.clone` | `Clone` trait |
| `std.eq` | `Eq` trait |
| `std.hash` | `Hash` trait |
| `std.ord` | `Ord` trait |
| `std.string` | `String` (heap-owning) |
| `std.arena` | `Arena`, arena allocation |
| `std.arena.default` | `DefaultAllocator` |
| `std.io` | `print`, `println`, I/O primitives |
| `std.io.file` | `File`, `BufferedReader`, `BufferedWriter` |
| `std.io.ring` | `IoUring` submission/completion queues |
| `std.collections` | `Vec<T, A>`, `HashMap`, `StringMap` |
| `std.sync` | `Mutex`, `AtomicI64`, `AtomicU64` |
| `std.channel` | `Channel<T>`, `UnboundedChannel<T>` |
| `std.thread` | `Thread::spawn`, `Thread::join` |
| `std.process` | `Command` execution |
| `std.http` | HTTP client (`connect`, `send`, `recv`, `close`) |
| `std.json` | JSON parser/writer |
| `std.json.json` | `JsonParser`, `JsonWriter`, `JsonArray`, `JsonObject` |
| `std.net` | Network primitives |
| `std.time` | `sleep_ms`, `sleep_nanos`, clock utilities |
| `std.simd` | Vector intrinsics (`v_load`, `v_store`, `v_fma`, etc.) |

The prelude implicitly imports `Ptr`, `Option`, `Result`, `Status`, `DefaultAllocator`, and `print`. These names are available without explicit imports in any source file.

---

## 13. FFI and Unsafe

### 13.1 Extern Functions

`extern fn` declares a function with C ABI linkage. The compiler does not generate a body. At link time, the symbol must be provided by an external object file. Only the following types may cross the FFI boundary: `i8` through `i64`, `u8` through `u64`, `f32`, `f64`, `bool`, `Ptr<T>`, `fn(T₁,...,Tₙ) -> R`. Attempting to use any other type in an extern function signature is a compile-time error.

### 13.2 `@export`

The `@export` attribute suppresses name mangling. An exported function can be called from C code by its declared name.

### 13.3 Unsafe Blocks

Within `unsafe { ... }`, the following operations are permitted:
- Raw pointer arithmetic on `Ptr<T>`
- `reinterpret_cast<T>(expr)` — reinterpret the bytes of `expr` as type `T`
- Dereferencing pointers without bounds or validity checks
- Calling functions not annotated with safety contracts

The compiler does not verify memory safety within `unsafe` blocks. Safety is the programmer's responsibility.

---

## Appendix A: Complete EBNF Grammar

```ebnf
(* Source file *)
COMPILATION_UNIT = [ PACKAGE_DECL ], { IMPORT_DECL }, { ITEM } ;

(* Declarations *)
PACKAGE_DECL    = "package", IDENTIFIER, { ".", IDENTIFIER }, ";" ;
IMPORT_DECL     = "import", PATH, [ ".", ( "*" | "{", IDENTIFIER, { ",", IDENTIFIER }, "}" ) ], ";" ;
PATH            = IDENTIFIER, { ".", IDENTIFIER } ;
ITEM            = FN_DECL | STRUCT_DECL | ENUM_DECL | CONST_DECL
                | GLOBAL_DECL | TRAIT_DECL | IMPL_BLOCK | CONCEPT_DECL
                | EXTERN_FN_DECL ;

(* Functions *)
FN_DECL         = { ATTRIBUTE }, [ "pub" ], "fn", IDENTIFIER,
                  [ "<", GENERIC_PARAMS, ">" ],
                  "(", [ PARAMS ], ")", [ "->", TYPE ],
                  { CONTRACT_CLAUSE }, BLOCK ;
PARAMS          = PARAM, { ",", PARAM } ;
PARAM           = PATTERN, ":", TYPE ;
EXTERN_FN_DECL  = "extern", "fn", IDENTIFIER, "(", [ PARAMS ], ")", [ "->", TYPE ],
                  { CONTRACT_CLAUSE }, ";" ;

(* Generics *)
GENERIC_PARAMS  = IDENTIFIER, { ",", IDENTIFIER } ;

(* Types *)
TYPE            = PRIMITIVE | PtrType | RefType | ArrayType
                | TupleType | FnPtrType | NamedType ;
PRIMITIVE       = "i8"|"i16"|"i32"|"i64"|"u8"|"u16"|"u32"|"u64"
                | "usize"|"f32"|"f64"|"bool"|"char"|"()" ;
PtrType         = "Ptr", "<", TYPE, ">" ;
RefType         = "&", [ "mut" ], TYPE ;
ArrayType       = "[", TYPE, ";", EXPRESSION, "]" ;
TupleType       = "(", [ TYPE, { ",", TYPE } ], ")" ;
FnPtrType       = "fn", "(", [ TYPE, { ",", TYPE } ], ")", [ "->", TYPE ] ;
NamedType       = PATH, [ "<", TYPE, { ",", TYPE }, ">" ] ;

(* Declarations *)
STRUCT_DECL     = { ATTRIBUTE }, "struct", IDENTIFIER, [ "<", GENERIC_PARAMS, ">" ],
                  "{", { STRUCT_FIELD }, "}" ;
STRUCT_FIELD    = { ATTRIBUTE }, IDENTIFIER, ":", TYPE, "," ;
ENUM_DECL       = "enum", IDENTIFIER, [ "<", GENERIC_PARAMS, ">" ],
                  "{", { ENUM_VARIANT, "," }, "}" ;
ENUM_VARIANT    = IDENTIFIER, [ "(", TYPE, { ",", TYPE }, ")" ] ;
CONST_DECL      = "const", IDENTIFIER, ":", TYPE, "=", EXPRESSION, ";" ;
GLOBAL_DECL     = "global", IDENTIFIER, ":", TYPE, "=", EXPRESSION, ";" ;
TRAIT_DECL      = "trait", IDENTIFIER, [ "<", GENERIC_PARAMS, ">" ],
                  "{", { TRAIT_METHOD }, "}" ;
TRAIT_METHOD    = { ATTRIBUTE }, "fn", IDENTIFIER, "(", [ PARAMS ], ")", [ "->", TYPE ], ";" ;
IMPL_BLOCK      = "impl", [ GENERIC_PARAMS ], [ PATH, "for" ], TYPE,
                  "{", { IMPL_ITEM }, "}" ;
IMPL_ITEM       = FN_DECL | TRAIT_METHOD ;
CONCEPT_DECL    = "concept", IDENTIFIER, "(", TYPE, ")", "requires", "(", EXPRESSION, ")", ";" ;

(* Statements *)
BLOCK           = "{", { STMT }, "}" ;
STMT            = LET_STMT | LET_ELSE_STMT | ASSIGN_STMT
                | IF_STMT | WHILE_STMT | FOR_STMT | LOOP_STMT
                | MATCH_STMT | RETURN_STMT | UNSAFE_BLOCK
                | REGION_STMT | EXPR_STMT ;

(* Expressions *)
EXPR_STMT       = EXPRESSION, ";" ;
LET_STMT        = "let", [ "mut" ], PATTERN, [ ":", TYPE ], "=", EXPRESSION, ";" ;
LET_ELSE_STMT   = "let", PATTERN, "=", EXPRESSION, "else", BLOCK ;
ASSIGN_STMT     = EXPRESSION, ASSIGN_OP, EXPRESSION, ";" ;
IF_STMT         = "if", EXPRESSION, BLOCK, [ "else", ( BLOCK | IF_STMT ) ] ;
WHILE_STMT      = "while", EXPRESSION, BLOCK ;
FOR_STMT        = "for", IDENTIFIER, "in", EXPRESSION, BLOCK ;
LOOP_STMT       = "loop", BLOCK ;
MATCH_STMT      = "match", EXPRESSION, "{", { MATCH_ARM, "," }, "}" ;
MATCH_ARM       = PATTERN, [ "if", EXPRESSION ], "=>", ( BLOCK | EXPRESSION, "," ) ;
RETURN_STMT     = "return", [ EXPRESSION ], ";" ;
UNSAFE_BLOCK    = "unsafe", BLOCK ;
REGION_STMT     = ( "with", "region", IDENTIFIER, BLOCK )
                | ( "region", "(", STRING_LIT, ")", BLOCK ) ;

(* Patterns *)
PATTERN         = "_" | PATTERN_LIT | PATTERN_BIND | PATTERN_VARIANT
                | PATTERN_TUPLE | PATTERN_STRUCT | PATTERN_OR ;
PATTERN_LIT     = LITERAL ;
PATTERN_BIND    = [ "mut" ], IDENTIFIER ;
PATTERN_VARIANT = PATH, "(", [ PATTERN, { ",", PATTERN } ], ")" ;
PATTERN_TUPLE   = "(", [ PATTERN, { ",", PATTERN } ], ")" ;
PATTERN_STRUCT  = PATH, "{", [ FIELD_PAT, { ",", FIELD_PAT } ], "}" ;
PATTERN_OR      = PATTERN, "|", PATTERN ;

(* Contracts *)
CONTRACT_CLAUSE = ( "requires" | "ensures" ), EXPRESSION, ";" ;

(* Attributes *)
ATTRIBUTE       = "@", IDENTIFIER, [ "(", ATTRIBUTE_ARGS, ")" ] ;
ATTRIBUTE_ARGS  = EXPRESSION, { ",", EXPRESSION } ;

(* Identifiers and literals *)
IDENTIFIER      = ( LETTER | "_" ), { LETTER | DIGIT | "_" } ;
LITERAL         = INTEGER_LIT | FLOAT_LIT | "true" | "false" | CHAR_LIT | STRING_LIT ;
```

---

## Appendix B: Operator Precedence (Complete)

| Prec | Operators | Assoc | Category |
|------|-----------|-------|----------|
| 17 | `.` | left | Field/method access |
| 16 | `()` `[]` | left | Call, index |
| 15 | `-` `!` `*` `&` | right | Unary |
| 14 | `as` | left | Type cast |
| 13 | `*` `/` `%` | left | Multiplicative |
| 12 | `+` `-` | left | Additive |
| 11 | `<<` `>>` | left | Shift |
| 10 | `&` | left | Bitwise AND |
| 9 | `^` | left | Bitwise XOR |
| 8 | `\|` | left | Bitwise OR |
| 7 | `<` `<=` `>` `>=` | left | Relational |
| 6 | `==` `!=` | left | Equality |
| 5 | `&&` | left | Logical AND |
| 4 | `\|\|` | left | Logical OR |
| 3 | `=` `+=` `-=` `*=` `/=` `%=` `<<=` `>>=` | right | Assignment |
| 2 | `\|>` `\|?>` | left | Pipe |
| 1 | `@` | left | Matmul |

---

## Appendix C: Compiler CLI

```
saltc <file.salt> [-o <path>] [flags]

  --release              Optimizations enabled (default: debug)
  --binary               Produce native executable (Mach-O, ELF, or PE)
  -c                     Produce .o object file
  --target <target>      Target platform: macos, linux-arm64, windows, keuos, keuos-x86_64
  --lib                  Library mode (no main entry point required)
  --sip                  Mode B SIP safety enforcement
  --verify               Enable contract verification (default: on)
  --danger-no-verify     Skip all verification (debug builds only)
  --skip-scan            Skip import dependency scanning
  --emit-sir             Emit SIR as JSON for tooling
  -g, --debug-info       Emit DWARF debug information
  --disable-alias-scopes Suppress LLVM alias scope metadata
  -o <path>              Output path (MLIR by default, or binary with --binary)
```

---

## Appendix D: Error Codes

The canonical registry is `salt-front/src/errors.rs`; `saltc --explain <code>`
renders its explanations. Compile-stage failures print an `[E003]` banner
followed by the root cause, which may carry its own refinement code (`[E002]`,
`[E009]`, `[E011]`). Warnings reuse the domain number with a `W` prefix and
never abort compilation.

| Code | Category | Emitted when |
|------|----------|--------------|
| E001 | File I/O | a source/output/SIR file cannot be read or written |
| E002 | Syntax | source fails to parse or uses invalid syntax |
| E003 | Compilation | comptime evaluation or MLIR lowering fails (banner) |
| E004 | CLI usage | missing or unknown flags/arguments |
| E005 | Binary synthesis | MLIR-to-native-binary pipeline fails |
| E006 | Object compilation | MLIR-to-object pipeline fails |
| E007 | Internal compiler error | compiler bug or disabled safety flag |
| E008 | Imports/modules | imported module cannot be resolved (fatal path) |
| E009 | Verification | a Z3 contract/invariant/ownership proof fails |
| E010 | Target triple | unknown `--target` value |
| E011 | Deferred policy | `--deny-deferred` sees deferred checks |
| W008 | Imports/modules (warning) | unresolvable import; compilation continues |
