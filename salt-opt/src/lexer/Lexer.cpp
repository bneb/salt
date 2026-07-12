#include "Lexer.h"
#include <cctype>
#include <iostream>

namespace salt {

Lexer::Lexer(std::string_view source, llvm::SourceMgr &sourceMgr)
    : curPtr(source.begin()), bufferStart(source.begin()),
      bufferEnd(source.end()), sourceMgr(sourceMgr) {
  curToken = lex(); // Prime the first token
}

Token Lexer::lex() {
  skipWhitespace();

  if (curPtr >= bufferEnd) {
    return Token{Tok_Eof, std::string_view(curPtr, 0),
                 llvm::SMLoc::getFromPointer(curPtr)};
  }

  const char *startPtr = curPtr;
  char charVal = *curPtr++;

  // Identifiers and Keywords
  if (std::isalpha(charVal) || charVal == '_') {
    while (curPtr < bufferEnd && (std::isalnum(*curPtr) || *curPtr == '_')) {
      curPtr++;
    }
    std::string_view spelling(startPtr, curPtr - startPtr);
    TokenKind kind = Tok_Identifier;

    if (spelling == "fn")
      kind = Tok_Fn;
    else if (spelling == "return")
      kind = Tok_Return;
    else if (spelling == "region")
      kind = Tok_Region;
    else if (spelling == "requires")
      kind = Tok_Requires;
    else if (spelling == "ensures")
      kind = Tok_Ensures;
    else if (spelling == "must")
      kind = Tok_Must;
    else if (spelling == "match")
      kind = Tok_Match;
    else if (spelling == "Ok")
      kind = Tok_Ok;
    else if (spelling == "Err")
      kind = Tok_Err;
    else if (spelling == "struct")
      kind = Tok_Struct; // [New]
    else if (spelling == "or")
      kind = Tok_Or; // [New]

    return Token{kind, spelling, llvm::SMLoc::getFromPointer(startPtr)};
  }

  // Numbers (Integers only for MVP)
  if (std::isdigit(charVal)) {
    while (curPtr < bufferEnd && std::isdigit(*curPtr)) {
      curPtr++;
    }
    return Token{Tok_IntLiteral, std::string_view(startPtr, curPtr - startPtr),
                 llvm::SMLoc::getFromPointer(startPtr)};
  }

  // String Literals
  if (charVal == '"') {
    while (curPtr < bufferEnd && *curPtr != '"') {
      curPtr++;
    }
    if (curPtr < bufferEnd)
      curPtr++; // Consume closing quote
    return Token{Tok_StringLiteral,
                 std::string_view(startPtr, curPtr - startPtr),
                 llvm::SMLoc::getFromPointer(startPtr)};
  }

  // Operators and Punctuation
  switch (charVal) {
  case '.':
    return Token{Tok_Dot, ".", llvm::SMLoc::getFromPointer(startPtr)}; // [New]
  case '(':
    return Token{Tok_LParen, "(", llvm::SMLoc::getFromPointer(startPtr)};
  case ')':
    return Token{Tok_RParen, ")", llvm::SMLoc::getFromPointer(startPtr)};
  case '{':
    return Token{Tok_LBrace, "{", llvm::SMLoc::getFromPointer(startPtr)};
  case '}':
    return Token{Tok_RBrace, "}", llvm::SMLoc::getFromPointer(startPtr)};
  case ',':
    return Token{Tok_Comma, ",", llvm::SMLoc::getFromPointer(startPtr)};
  case ':':
    return Token{Tok_Colon, ":", llvm::SMLoc::getFromPointer(startPtr)};
  case ';':
    return Token{Tok_Semicolon, ";", llvm::SMLoc::getFromPointer(startPtr)};
  case '+':
    return Token{Tok_Plus, "+", llvm::SMLoc::getFromPointer(startPtr)};
  case '*':
    return Token{Tok_Mul, "*", llvm::SMLoc::getFromPointer(startPtr)};
  case '/':
    return Token{Tok_Div, "/", llvm::SMLoc::getFromPointer(startPtr)};
  case '=':
    if (curPtr < bufferEnd && *curPtr == '=') { // ==
      curPtr++;
      return Token{Tok_EqEq, "==", llvm::SMLoc::getFromPointer(startPtr)};
    }
    return Token{Tok_Eq, "=", llvm::SMLoc::getFromPointer(startPtr)}; // =
  case '!':
    if (curPtr < bufferEnd && *curPtr == '=') { // !=
      curPtr++;
      return Token{Tok_Ne, "!=", llvm::SMLoc::getFromPointer(startPtr)};
    }
    break;
  case '-':
    if (curPtr < bufferEnd && *curPtr == '>') { // ->
      curPtr++;
      return Token{Tok_Arrow, "->", llvm::SMLoc::getFromPointer(startPtr)};
    }
    return Token{Tok_Minus, "-", llvm::SMLoc::getFromPointer(startPtr)};
  case '|':
    if (curPtr + 1 < bufferEnd && *curPtr == '?' &&
        *(curPtr + 1) == '>') { // |?>
      curPtr += 2;
      return Token{Tok_RailwayPipe, "|?>",
                   llvm::SMLoc::getFromPointer(startPtr)};
    }
    if (curPtr < bufferEnd && *curPtr == '>') { // |>
      curPtr++;
      return Token{Tok_Pipe, "|>", llvm::SMLoc::getFromPointer(startPtr)};
    }
    break; // Handle single | if needed, or error
  case '<':
    return Token{Tok_Lt, "<", llvm::SMLoc::getFromPointer(startPtr)};
  case '>':
    return Token{Tok_Gt, ">", llvm::SMLoc::getFromPointer(startPtr)};
  }

  return Token{Tok_Error, std::string_view(startPtr, 1),
               llvm::SMLoc::getFromPointer(startPtr)};
}

void Lexer::skipWhitespace() {
  while (curPtr < bufferEnd) {
    if (std::isspace(*curPtr)) {
      curPtr++;
    } else if (*curPtr == '/' && curPtr + 1 < bufferEnd &&
               *(curPtr + 1) == '/') {
      // Comment
      curPtr += 2;
      while (curPtr < bufferEnd && *curPtr != '\n' && *curPtr != '\r')
        curPtr++;
    } else {
      break;
    }
  }
}

} // namespace salt
