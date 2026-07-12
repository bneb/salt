# Salt RAII & Resource Management

> Salt tracks resource ownership at compile time. The Drop trait provides deterministic cleanup for files, sockets, and allocations.

---

## Current Status

Salt provides **manual resource management** with ownership tracking:

- **Move semantics** prevent use-after-free (compile-time error)
- **`Ptr<T>`** with provenance tracking ensures pointer validity
- **Heap allocators** (`HeapAllocator`, `ArenaAllocator`) have explicit `free`/`destroy` methods
- **File/Socket types** have explicit `.close()` methods

```salt
let file = File::open("data.txt")?;
let content = file.read_all()?;
file.close();  // Explicit cleanup
```

---

## Drop Trait Design

### Planned Interface

```salt
trait Drop {
    fn drop(&mut self);
}
```

Types implementing `Drop` will have their `drop()` method called automatically when the variable goes out of scope — exactly once, at the closing `}` of the innermost enclosing block.

### Example: File with RAII

```salt
impl Drop for File {
    fn drop(&mut self) {
        if self.fd >= 0 {
            sys_close(self.fd);
            self.fd = -1;
        }
    }
}

fn process_file() -> Result<i64> {
    let file = File::open("data.txt")?;
    let data = file.read_all()?;
    return Result::Ok(data.len());
    // file.drop() called automatically here
}
```

### Example: Mutex Guard

```salt
struct MutexGuard {
    mutex: &mut Mutex
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

impl Mutex {
    fn lock_guard(&mut self) -> MutexGuard {
        self.lock();
        return MutexGuard { mutex: self };
    }
}

fn critical_section(m: &mut Mutex) {
    let _guard = m.lock_guard();
    // ... critical work ...
    // _guard.drop() releases the lock automatically
}
```

---

## Destructor Ordering

- **Reverse declaration order**: Variables are dropped in reverse order of their declaration within a block
- **Nested scopes**: Inner scopes complete their drops before outer scopes
- **Early return**: All live variables are dropped before `return`
- **Move suppresses drop**: If a variable has been moved, no drop is emitted

```salt
fn example() {
    let a = Resource::new("first");   // Dropped third
    let b = Resource::new("second");  // Dropped second
    {
        let c = Resource::new("inner");  // Dropped first (inner scope)
    }  // c.drop() here
}  // b.drop() then a.drop() here
```

---

## Implementation: Codegen Integration

Drop insertion happens during MLIR emission:

1. **Registration**: When a `let` binding creates a type that implements `Drop`, register it in the current scope's drop list
2. **Scope exit**: At `}`, emit `drop()` calls for all registered variables in reverse order
3. **Move tracking**: When a variable is moved, remove it from the drop list (already consumed)
4. **Early return**: Before `return`, emit drops for all live variables in enclosing scopes
5. **Branch merging**: After `if/else`, the union of drops from both branches determines what's still live

---

## Comparison with Rust

| Feature | Salt | Rust |
|---------|------|------|
| Deterministic drop | ✅ | ✅ |
| Drop order | Reverse declaration | Reverse declaration |
| `Drop` trait | Planned | `std::ops::Drop` |
| Drop on move | Suppressed | Suppressed |
| `ManuallyDrop` | Not yet | `std::mem::ManuallyDrop` |
| `std::mem::forget` | Not yet | Available |
| Drop in `if/else` | Union tracking | Union tracking |

---

## Current Resource Patterns

Until the Drop trait is fully implemented, Salt resources use **explicit cleanup**:

| Resource | Cleanup Method |
|----------|---------------|
| `File` | `.close()` |
| `TcpListener` | `.close()` |
| `TcpStream` | `.close()` |
| `Mutex` | `.destroy()` |
| `Vec<T>` | `.free()` |
| `String` | `.free()` |
| Arena allocator | `.destroy()` |
