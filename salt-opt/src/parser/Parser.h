#ifndef SALT_PARSER_H
#define SALT_PARSER_H

#include "../lexer/Lexer.h"
#include "mlir/IR/Builders.h"
#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/Location.h"
#include <map>
#include <string>

namespace salt {

class Parser {
public:
  Parser(Lexer &lexer, mlir::OpBuilder &builder, mlir::MLIRContext *context)
      : lexer(lexer), builder(builder), context(context) {}

  /// Parse a complete module
  mlir::ModuleOp parseModule();

private:
  Lexer &lexer;
  mlir::OpBuilder &builder;
  mlir::MLIRContext *context;

  // Symbol Table for variable lookup (scoped)
  std::map<std::string, mlir::Value> symbolTable;

  // Helper to handle locations
  mlir::Location loc(Token tok);
  mlir::Location curLoc() { return loc(lexer.getLastToken()); }

  // Parsing methods
  void parseFunction();
  void parseStruct();    // [New]
  void parseBlockBody(); // Parse inside {}

  // Type parsing
  mlir::Type parseType(); // [New] Enhanced type parsing

  // Expression parsing
  mlir::Value parseExpression();
  mlir::Value parsePrimary();
  mlir::Value parseBinOpRHS(int exprPrec, mlir::Value lhs);
  mlir::Value parseIdentifierExpr();
  mlir::Value parseParenExpr();
  mlir::Value parseBlockLambda();              // { x -> ... }
  mlir::Value parseMatchExpr(mlir::Value val); // [New] match(val, { patterns })

  // Helpers
  void consume(TokenKind kind);
  bool is(TokenKind kind) { return lexer.getLastToken().kind == kind; }

  // Handle syntax sugar
  mlir::Value handlePipe(mlir::Value lhs, mlir::Value rhs);
  mlir::Value handleRailwayPipe(mlir::Value lhs, mlir::Value rhs);
};

} // namespace salt

#endif // SALT_PARSER_H
