# 🏛️ Salt Standard Library

70+ modules providing production-ready systems primitives. No external dependencies.

## Module Map

### Core (`std.core`)
Arena allocator, `Result<T,E>`, `Option<T>`, `Ptr<T>`, `Box<T>`, `StringView`, iterators, slices, memory operations, slab allocator, formatting.

### Collections (`std.collections`)
`Vec<T,A>` (generic allocator), `HashMap<K,V>` (Swiss-table with bit-group probing).

### String (`std.string`)
Flat `String` type (`{data: Ptr<u8>, len: i64, cap: i64}`) with f-string support, `Eq`, `Writer` impl.

### I/O (`std.io`)
`println`, `Writer` trait, `BufferedWriter`, `BufferedReader`, `File` (read/write/write_all), platform-specific arch support.

### Networking (`std.net`)
`TcpListener`, `TcpStream`, `Poller` (kqueue), FFI bridge.

### HTTP (`std.http`)
HTTP client (`get_raw`, `connect`, send/recv), response builder, request parser.

### Threading (`std.thread`)
`Thread::spawn`, `Thread::join` — pthread-backed thread creation.

### Synchronization (`std.sync`)
`Mutex` (pthread), `AtomicI64` (C11 atomics: load, store, fetch_add, compare_exchange).

### Process (`std.process`)
`Command` builder for subprocess execution via `posix_spawn`.

### Environment (`std.env`)
Environment variable access (`get`, `set`, `has`).

### JSON (`std.json`)
JSON parsing and value access.

### Path (`std.path`)
File path operations (`join`, `extension`, `basename`, `dirname`).

### Math & SIMD (`std.math`, `std.simd`)
- **Math**: Vectorized transcendentals (`vexp`, `vrelu`, `powf`), minimax polynomial approximations
- **SIMD**: `f32x4`, `i32x4`, `v_fma`, `v_add` — direct NEON register mapping

### Linear Algebra (`std.linalg`)
`Tensor<T, [Dim]>`, `matmul`, `transpose`. Const generics enforce geometric correctness at compile-time.

### Neural Networks (`std.nn`)
Activations (`relu`, `sigmoid`, `softmax`), loss functions (`cross_entropy`, `mse`), tensor ops.

### Autograd (`std.autograd`)
Compile-time automatic differentiation. The compiler synthesizes the backward pass from the forward graph.

### Crypto (`std.crypto`)
TLS bridge with FFI bindings.

### Regex (`std.regex`)
Regular expression matching with FFI bridge.

### Filesystem (`std.fs`)
File system operations: `exists`, `remove_file`, `remove_dir`, `rename_path`, `create_dir`. POSIX FFI bridges with `@trusted` wrappers accepting `StringView`.

### Encoding (`std.encoding`)
Pure Salt encoding utilities: Base64 (`base64_encode`, `base64_encoded_len`) and Hex (`hex_encode`, `hex_encoded_len`). No FFI — fully verifiable.

### Utilities
| Module | Contents |
|--------|----------|
| `std.random` | Xorshift128+ PRNG |
| `std.time` | High-resolution timing |
| `std.fmt` | Formatting traits |
| `std.eq` | Equality trait |
| `std.hash` | Hash trait |
| `std.os` | OS-level syscall wrappers |
| `std.sys` | System module |
| `std.test` | Test framework |
| `std.log` | Logging utilities |
| `std.tensor` | Tensor primitives |
| `std.mem` | Memory safety (`SafeView<T>`, `mmap_view`) |
