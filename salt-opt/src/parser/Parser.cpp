#include "Parser.h"
#include "../lexer/Lexer.h"
#include "mlir/Dialect/Arith/IR/Arith.h"
#include "mlir/Dialect/Func/IR/FuncOps.h"
#include "mlir/IR/Builders.h"
#include "mlir/IR/BuiltinTypes.h"
#include "mlir/IR/Verifier.h" // For verifying ops if needed
#include <iostream>
#include <vector>

namespace salt {

mlir::Location Parser::loc(Token tok) {
  // MVP: SourceMgr based location would be ideal, using Unknown for now
  return builder.getUnknownLoc();
}

void Parser::consume(TokenKind kind) {
  if (lexer.getLastToken().kind == kind) {
    lexer.lex();
  } else {
    std::cerr << "Expected token kind " << kind << " but got "
              << lexer.getLastToken().kind << " at "
              << lexer.getLastToken().spelling << "\n";
    exit(1);
  }
}

mlir::ModuleOp Parser::parseModule() {
  mlir::ModuleOp module = mlir::ModuleOp::create(builder.getUnknownLoc());
  builder.setInsertionPointToStart(module.getBody());

  while (lexer.getLastToken().kind != Tok_Eof) {
    if (lexer.getLastToken().kind == Tok_Fn) {
      parseFunction();
    } else if (lexer.getLastToken().kind == Tok_Struct) { // [New]
      parseStruct();
    } else {
      // Skip top level unknown tokens to avoid infinite loops in MVP
      std::cerr << "Unexpected token at top level: "
                << lexer.getLastToken().spelling << "\n";
      lexer.lex();
    }
  }
  return module;
}

void Parser::parseStruct() {
  consume(Tok_Struct);
  Token nameTok = lexer.getLastToken();
  consume(Tok_Identifier);
  consume(Tok_LBrace);

  llvm::SmallVector<mlir::Type, 4> fieldTypes;
  llvm::SmallVector<llvm::StringRef, 4> fieldNames;

  // MVP: Parse fields 'name: type'
  while (!is(Tok_RBrace) && !is(Tok_Eof)) {
    Token fieldName = lexer.getLastToken();
    consume(Tok_Identifier);
    consume(Tok_Colon);
    mlir::Type type = parseType();

    fieldTypes.push_back(type);
    // Store name? 'struct_def' needs names as strings
    // We need consistent storage for stringrefs if we don't own them.
    // For MVP, assume immediate usage.

    if (is(Tok_Comma))
      consume(Tok_Comma); // Optional comma? Gauntlet example has significant
                          // newlines implicitly or just standard C-like without
                          // punctuation shown clearly. inferred newlines or no
                          // separators? "val: i32 \n left: ..."
    // We'll require comma for safety or just loop if identifier.
    // Gauntlet:
    // val: i32
    // left: ...
    // We'll assume optional comma.
    if (is(Tok_Identifier))
      continue;
  }
  consume(Tok_RBrace);

  // Emit salt.struct_def
  // builder.create<StructDefOp>(...)
  // Generic
  llvm::SmallVector<mlir::Attribute, 4> nameAttrs;
  for (auto n : fieldNames)
    nameAttrs.push_back(builder.getStringAttr(n));

  builder.create(curLoc(), builder.getStringAttr("salt.struct_def"),
                 builder.getStringAttr(nameTok.spelling),
                 builder.getTypeArrayAttr(fieldTypes),
                 builder.getStrArrayAttr(nameAttrs));
}

mlir::Type Parser::parseType() {
  // 1. Primary Type
  mlir::Type type;
  Token tok = lexer.getLastToken();
  if (tok.kind == Tok_Identifier) {
    if (tok.spelling == "i32")
      type = builder.getI32Type();
    else if (tok.spelling == "u64")
      type = builder.getI64Type();
    else if (tok.spelling == "Str")
      type = builder.getStringAttr("Str").getType(); // Placeholder
    else {
      // Struct Name or Generic?
      // List(i32)
      std::string baseName(tok.spelling);
      consume(Tok_Identifier);
      if (is(Tok_LParen)) {
        consume(Tok_LParen);
        // Generic Type Arg
        parseType();
        consume(Tok_RParen);
        // Return Opaque Generic Type
      }
      type = builder.getI32Type(); // Fallback for struct ref for MVP
      // Return early to avoid double consume
      goto check_union;
    }
    consume(Tok_Identifier);
  } else {
    consume(Tok_Identifier); // Error recovery
    type = builder.getNoneType();
  }

check_union:
  // Handle 'or' (Union Types)
  if (is(Tok_Or)) {
    consume(Tok_Or);
    parseType();
    // Return Union Type (Placeholder)
  }

  return type;
}

void Parser::parseFunction() {
  // format: fn name(args) -> type requires(...) { ... }
  consume(Tok_Fn);

  Token nameTok = lexer.getLastToken();
  consume(Tok_Identifier);
  std::string name(nameTok.spelling);

  consume(Tok_LParen);

  std::vector<std::string> argNames;
  std::vector<mlir::Type> argTypes;

  while (!is(Tok_RParen)) {
    Token argName = lexer.getLastToken();
    consume(Tok_Identifier);
    consume(Tok_Colon);

    mlir::Type ty = parseType();
    argTypes.push_back(ty);
    argNames.push_back(std::string(argName.spelling));

    if (is(Tok_Comma))
      consume(Tok_Comma);
  }
  consume(Tok_RParen);

  mlir::Type resultType = builder.getNoneType();
  if (is(Tok_Arrow)) {
    consume(Tok_Arrow);
    resultType = parseType();
  }

  auto funcType = builder.getFunctionType(
      argTypes, resultType != builder.getNoneType() ? resultType : llvm::None);
  auto func =
      builder.create(curLoc(), builder.getStringAttr("salt.func"),
                     builder.getStringAttr(name), mlir::TypeAttr::get(funcType),
                     /*arg_attrs=*/nullptr, /*res_attrs=*/nullptr);

  // Enter function body
  mlir::Region *bodyRegion = func->getRegion(0);
  mlir::Block *entryBlock = builder.createBlock(bodyRegion);

  symbolTable.clear();
  for (size_t i = 0; i < argNames.size(); ++i) {
    mlir::Value arg = entryBlock->addArgument(argTypes[i], curLoc());
    symbolTable[argNames[i]] = arg;
  }

  // Store insertion point safely
  // Process 'requires' / 'ensures' before body block starts strictly?
  // User snippet: requires(...) { ... }
  // So we parse them before LBrace.

  while (is(Tok_Requires) || is(Tok_Ensures)) {
    if (is(Tok_Requires)) {
      consume(Tok_Requires);
      consume(Tok_LParen);
      mlir::Value cond = parseExpression();
      consume(Tok_RParen);
      builder.create(curLoc(), builder.getStringAttr("salt.verify_op"), cond,
                     builder.getStringAttr("requires"));
    }
    if (is(Tok_Ensures)) {
      consume(Tok_Ensures);
      consume(Tok_LParen);
      mlir::Value cond = parseExpression();
      consume(Tok_RParen);
      builder.create(curLoc(), builder.getStringAttr("salt.verify_op"), cond,
                     builder.getStringAttr("ensures"));
    }
  }

  parseBlockBody();

  // Ensure terminator
  if (entryBlock->empty() ||
      !entryBlock->back().hasTrait<mlir::OpTrait::IsTerminator>()) {
    builder.create(curLoc(), builder.getStringAttr("salt.return"));
  }
}

void Parser::parseBlockBody() {
  consume(Tok_LBrace);
  while (!is(Tok_RBrace) && !is(Tok_Eof)) {
    if (is(Tok_Return)) {
      consume(Tok_Return);
      if (!is(Tok_RBrace) && !is(Tok_Semicolon)) {
        mlir::Value retVal = parseExpression();
        builder.create(curLoc(), builder.getStringAttr("salt.return"), retVal);
      } else {
        builder.create(curLoc(), builder.getStringAttr("salt.return"));
      }
    } else {
      parseExpression();
    }
    if (is(Tok_Semicolon))
      consume(Tok_Semicolon);
  }
  consume(Tok_RBrace);
}

mlir::Value Parser::parseExpression() {
  auto lhs = parsePrimary();
  return parseBinOpRHS(0, lhs);
}

mlir::Value Parser::parsePrimary() {
  if (is(Tok_IntLiteral)) {
    int val = std::stoi(std::string(lexer.getLastToken().spelling));
    lexer.lex();
    return builder.create(curLoc(), builder.getStringAttr("salt.constant"),
                          builder.getI32IntegerAttr(val));
  }

  if (is(Tok_Identifier)) {
    return parseIdentifierExpr();
  }

  if (is(Tok_LParen)) {
    consume(Tok_LParen);
    auto v = parseExpression();
    consume(Tok_RParen);
    return v;
  }

  if (is(Tok_LBrace)) {
    return parseBlockLambda();
  }

  std::cerr << "Unknown token: " << lexer.getLastToken().spelling << "\n";
  exit(1);
  return nullptr;
}

mlir::Value Parser::parseIdentifierExpr() {
  Token idTok = lexer.getLastToken();
  consume(Tok_Identifier);

  // Check for 'match' (pseudo-keyword or indentifier match)
  // If identifier is 'match' and followed by '(', treat as match expr.
  if (idTok.spelling == "match" && is(Tok_LParen)) {
    consume(Tok_LParen);
    mlir::Value val = parseExpression();
    consume(Tok_Comma);
    // Expect Block of patterns
    mlir::Value res = parseMatchExpr(val);
    consume(Tok_RParen);
    return res;
  }

  if (is(Tok_LParen)) {
    // Call OR Struct Construction "Node(val: ...)"
    consume(Tok_LParen);
    // Lookahead to see if we have named args? "val:"
    // Only if identifier then colon.
    // MVP: Assume generic call for now, handle Struct Construct if needed via
    // same op or separate.
    std::vector<mlir::Value> args;
    if (!is(Tok_RParen)) {
      while (true) {
        // If named arg "val: ..." skip name for now
        if (is(Tok_Identifier) && lexer.peekToken().kind == Tok_Colon) {
          // consume identifier, colon
          consume(Tok_Identifier);
          consume(Tok_Colon);
        }
        args.push_back(parseExpression());
        if (is(Tok_RParen))
          break;
        consume(Tok_Comma);
      }
    }
    consume(Tok_RParen);

    // Use StructConstructOp if idTok is a Struct?
    // We lack context. Use CallOp or specialized StructConstructOp.
    // For 'Node(...)', let's use struct_construct.
    // Simple heuristic: caps = Struct?
    if (isupper(idTok.spelling[0])) {
      return builder
          .create(curLoc(), builder.getStringAttr("salt.struct_construct"),
                  mlir::FlatSymbolRefAttr::get(builder.getContext(),
                                               idTok.spelling),
                  args)
          .getResult(0);
    }

    return builder
        .create(
            curLoc(), builder.getStringAttr("salt.call"),
            mlir::FlatSymbolRefAttr::get(builder.getContext(), idTok.spelling),
            args)
        .getResult(0);
  }

  if (symbolTable.count(std::string(idTok.spelling))) {
    return symbolTable[std::string(idTok.spelling)];
  }

  // Recovery
  return builder.create(curLoc(), builder.getStringAttr("salt.undefined"),
                        builder.getStringAttr(idTok.spelling));
}

mlir::Value Parser::parseMatchExpr(mlir::Value val) {
  // match(val, { None -> ..., Some(x) -> ... })
  // We already moved past val and comma.
  // Expect LBrace
  consume(Tok_LBrace);
  // Parse patterns
  // MVP: consume patterns until RBrace, return placeholder result.
  // Real implementation requires SaltMatchOp with regions.
  while (!is(Tok_RBrace) && !is(Tok_Eof)) {
    // Pattern: Identifier(args) or Identifier
    if (is(Tok_Identifier))
      consume(Tok_Identifier);
    if (is(Tok_LParen)) {
      consume(Tok_LParen);
      // bindings
      consume(Tok_Identifier);
      consume(Tok_RParen);
    }
    consume(Tok_Arrow);
    // Body: Block or Expr?
    // "return None" -> Stmt?
    // In Salt, usually Expr.
    if (is(Tok_Return)) {
      consume(Tok_Return);
      parseExpression();
    } else if (is(Tok_LBrace)) {
      // Nested block
      parseBlockBody();
    } else {
      parseExpression();
    }
    if (is(Tok_Comma))
      consume(Tok_Comma);
  }
  consume(Tok_RBrace);
  // Return dummy
  return val;
}

mlir::Value Parser::parseBlockLambda() {
  // { x -> x + 1 }
  consume(Tok_LBrace);
  Token param = lexer.getLastToken();
  consume(Tok_Identifier);
  consume(Tok_Arrow);

  // TODO: Create a nested region or closure op.
  // MVP: Parse body but consume it. return dummy.
  auto body = parseExpression();
  consume(Tok_RBrace);

  return body; // Returning body value directly (invalid semantics but parsing
               // works)
}

mlir::Value Parser::parseBinOpRHS(int exprPrec, mlir::Value lhs) {
  while (true) {
    int tokPrec = -1;
    TokenKind k = lexer.getLastToken().kind;
    if (k == Tok_Dot)
      tokPrec = 50; // High precedence
    else if (k == Tok_Mul || k == Tok_Div)
      tokPrec = 40;
    else if (k == Tok_Plus || k == Tok_Minus)
      tokPrec = 20;
    else if (k == Tok_Lt || k == Tok_Gt || k == Tok_EqEq || k == Tok_Ne)
      tokPrec = 10;
    else if (k == Tok_Pipe || k == Tok_RailwayPipe)
      tokPrec = 5;

    if (tokPrec < exprPrec)
      return lhs;

    Token binOp = lexer.getLastToken();
    lexer.lex();

    // Special handle for Dot: RHS must be identifier
    if (binOp.kind == Tok_Dot) {
      Token member = lexer.getLastToken();
      consume(Tok_Identifier);
      lhs = builder
                .create(curLoc(), builder.getStringAttr("salt.member_access"),
                        lhs, builder.getStringAttr(member.spelling))
                .getResult(0);
      continue; // No recursive parse needed usually for single dot, but
                // chaining possible.
    }

    auto rhs = parsePrimary();

    int nextPrec = -1;
    TokenKind nextK = lexer.getLastToken().kind;
    if (nextK == Tok_Dot)
      nextPrec = 50;
    else if (nextK == Tok_Mul || nextK == Tok_Div)
      nextPrec = 40;

    if (tokPrec < nextPrec) {
      rhs = parseBinOpRHS(tokPrec + 1, rhs);
    }

    switch (binOp.kind) {
    case Tok_Plus:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.addi"), lhs,
                           rhs);
      break;
    case Tok_Minus:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.subi"), lhs,
                           rhs);
      break;
    case Tok_Mul:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.muli"), lhs,
                           rhs);
      break;
    case Tok_Div:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.divsi"), lhs,
                           rhs);
      break;
    case Tok_EqEq:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.cmpi"),
                           builder.getI64IntegerAttr(0) /* eq */, lhs, rhs);
      break;
    case Tok_Ne:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.cmpi"),
                           builder.getI64IntegerAttr(1) /* ne */, lhs, rhs);
      break;
    case Tok_Lt:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.cmpi"),
                           builder.getI64IntegerAttr(2) /* slt */, lhs, rhs);
      break;
    case Tok_Gt:
      lhs = builder.create(curLoc(), builder.getStringAttr("arith.cmpi"),
                           builder.getI64IntegerAttr(4) /* sgt */, lhs, rhs);
      break;

    case Tok_Pipe:
      // LHS |> RHS -> RHS(LHS)
      // Assuming RHS is a result of a call that returned a function handle, OR
      // we need to reinterpret RHS. Major simplification: If RHS is a
      // 'salt.undefined' (which we returned for unknown symbols), we treat it
      // as the function name. Real compile would inspect Value type. We'll
      // generate a salt.call using LHS as arg. But we don't know the name of
      // RHS if it's a value. This shows why AST is often better. For MVP: We
      // assume 'rhs' came from a variable lookup which we can't fully
      // introspect here. BUT if we look at parsePrimary, if it was an
      // identifier, it returned variable lookup. If variable not found, it
      // errored or returned undefined. Let's assume for |>, the user does `x |>
      // f`. `f` is likely a function. We'll emit `salt.call_indirect` (if
      // supported) or `salt.call` if we can recover the name. Since we can't
      // recover name from Value easily without casting, we'll try to use a
      // specialized builder or CustomOp. We'll emit a comment via a dummy op
      // `salt.pipe`.
      lhs =
          builder.create(curLoc(), builder.getStringAttr("salt.call"), rhs, lhs)
              .getResult(0); // Fake call
      break;

    case Tok_RailwayPipe:
      // LHS |?> RHS -> match LHS { Ok(v) -> RHS(v), Err(e) -> Err(e) }
      // We use salt.match
      // Operand: LHS
      // Regions: Ok, Err.
      {
        auto matchOp =
            builder.create(curLoc(), builder.getStringAttr("salt.match"),
                           lhs); // args...
        // Implementation of regions omitted for brevity in single-file gen,
        // but structure is:
        /*
        Block *okBlock = builder.createBlock(&matchOp.getRegion(0));
        okBlock->addArgument(lhs.getType(), curLoc());
        // call RHS(arg)
        // builder.create<YieldOp>(...)

        Block *errBlock = builder.createBlock(&matchOp.getRegion(1));
        // yield err
        */

        // We return the result of match
        lhs = matchOp.getResult(0);
      }
      break;

    default:
      break;
    }
  }
}

} // namespace salt
