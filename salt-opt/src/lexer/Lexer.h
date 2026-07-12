#ifndef SALT_LEXER_H
#define SALT_LEXER_H

#include <llvm/ADT/StringRef.h>
#include <llvm/Support/SMLoc.h>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace salt {

// Token definitions
enum TokenKind {
  Tok_Eof,
  Tok_Error,

  // Keywords
  Tok_Fn,
  Tok_Return,
  Tok_Region,
  Tok_Requires,
  Tok_Ensures,
  Tok_Must,
  Tok_Match,
  Tok_Ok,
  Tok_Err,
  Tok_Struct, // [New]
  Tok_Or,     // [New] for Union Types

  // Identifiers and Literals
  Tok_Identifier,
  Tok_IntLiteral,
  Tok_StringLiteral,

  // Punctuation
  Tok_LParen,
  Tok_RParen,
  Tok_LBrace,
  Tok_RBrace,
  Tok_Comma,
  Tok_Colon,
  Tok_Semicolon,
  Tok_Dot, // [New] for member access

  // Operators
  Tok_Arrow,       // ->
  Tok_Pipe,        // |>
  Tok_RailwayPipe, // |?>
  Tok_Plus,        // +
  Tok_Minus,       // -
  Tok_Mul,         // *
  Tok_Div,         // /
  Tok_Eq,          // =
  Tok_EqEq,        // ==
  Tok_Ne,          // !=
  Tok_Lt,          // <
  Tok_Gt,          // >
};

struct Token {
  TokenKind kind;
  std::string_view spelling;
  llvm::SMLoc loc;
};

class Lexer {
public:
  Lexer(std::string_view source, llvm::SourceMgr &sourceMgr);

  Token lex();
  Token getLastToken() const { return curToken; }

  // Helper for error reporting
  llvm::SMLoc getLoc() const { return curToken.loc; }

private:
  Token getNextToken();
  Token formToken(TokenKind kind, const char *tokEnd);

  // Skip whitespace and comments
  void skipWhitespace();

  const char *curPtr;
  const char *bufferStart;
  const char *bufferEnd;
  Token curToken;
  llvm::SourceMgr &sourceMgr;
};

} // namespace salt

#endif // SALT_LEXER_H
