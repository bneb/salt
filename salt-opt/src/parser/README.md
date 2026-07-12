# Salt Parser

**The Mission:** Construct the Abstract Syntax Tree (AST) from the token stream.

## Invariants
- **Recursive Descent:** Hand-written recursive descent parser for readability and easy error recovery.
- **Operator Precedence:** Uses "Precedence Climbing" for binary expressions.

## Components
| File | Role |
|------|------|
| [`Parser.h`](./Parser.h) | **API.** `class Parser` and AST node definitions. |
| [`Parser.cpp`](./Parser.cpp) | **Implementation.** `parsePrimary()`, `parseExpression()`. |
