# Salt Lexer

**The Mission:** Tokenize the raw source text into a stream of atomic units.

## Invariants
- **No Regex:** We use a hand-written state machine for maximum speed and zero dependencies.
- **Context-Free:** The lexer does not know about types or scopes; it only sees characters.

## Components
| File | Role |
|------|------|
| [`Lexer.h`](./Lexer.h) | **API.** `class Lexer` and `struct Token`. |
| [`Lexer.cpp`](./Lexer.cpp) | **Implementation.** The `gettok()` state machine. |
