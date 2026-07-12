# Salt Closure Capture Semantics

> Closures in Salt are monomorphized at compile time — no runtime dispatch, no heap-allocated closure objects.

---

## Current Status

Salt supports **first-class function references** — you can pass functions as values using `fn_name as i64` and invoke them across threads or FFI boundaries. True closure captures (where an anonymous function captures variables from its enclosing scope) are a planned feature.

### What Works Today

```salt
// First-class functions: pass by address
fn worker() {
    println("running on thread");
}

let handle = Thread::spawn(worker as i64);
handle.join();

// Function references in pipelines
let result = 5 |> square() |> double();
```

---

## Planned Capture Semantics

### By-Move (Default)

When a closure references a local variable, Salt will capture it **by move** — the value is transferred into the closure's environment struct. The original binding becomes consumed (use-after-move error).

```salt
// Planned syntax:
let name = String::from("Salt");
let greet = |x: i32| -> String {
    return f"{name}: {x}";   // `name` captured by-move
};
// name is consumed here — using it is a compile error
```

### By-Reference (`&`)

For closures that borrow data, a by-reference capture pattern:

```salt
// Planned:
let data = Vec::new();
let reader = |idx: i64| -> i64 {
    return data[idx];  // `data` captured by-ref
};
// data is still valid — reader borrows it
```

### Monomorphization

Closures are implemented as anonymous structs carrying their captured environment:

```
// Closure: |x: i32| -> i32 { return x + base; }
// Becomes:
struct __closure_1 { base: i32 }
fn __closure_1_call(self: &__closure_1, x: i32) -> i32 {
    return x + self.base;
}
```

Each unique closure gets its own monomorphized type — no dynamic dispatch, no `dyn Fn` equivalent. This matches Salt's zero-cost abstraction philosophy.

---

## Comparison with Rust

| Feature | Salt | Rust |
|---------|------|------|
| Capture default | By-move | By-ref (inferred) |
| Move capture | Default | Requires `move` keyword |
| Ref capture | Explicit `&` | Default (auto-inferred) |
| Closure traits | None (monomorphized) | `Fn`, `FnMut`, `FnOnce` |
| Boxing | Not needed | `Box<dyn Fn>` for type erasure |
| Runtime cost | Zero (all static) | Zero for concrete, box for erased |

## Trade-offs

- **Pro**: Zero overhead — every closure is fully monomorphized, inlined where possible
- **Pro**: No lifetime complexity — capture semantics follow move/borrow rules
- **Con**: Cannot store heterogeneous closures in a collection (no `dyn Fn`)
- **Con**: Code size may increase with heavy generic/closure use

---

## Implementation Path

1. Parser: Recognize `|params| -> ret { body }` syntax
2. AST: Closure nodes with free-variable analysis
3. Codegen: Generate environment struct + call function
4. Ownership: Track captured variables in move/borrow checker
