//===- SaltOps.cpp - Salt Operations Implementation ----------------------===//
//
// This file implements the Salt dialect operations.
//
//===----------------------------------------------------------------------===//

#include "SaltOps.h"
#include "SaltDialect.h"
#include "mlir/IR/Builders.h"
#include "mlir/IR/OpImplementation.h"

using namespace mlir;
using namespace salt;

//===----------------------------------------------------------------------===//
// CallOp - Symbol verification
//===----------------------------------------------------------------------===//

LogicalResult CallOp::verifySymbolUses(SymbolTableCollection &symbolTable) {
  // Check that the callee attribute was specified.
  auto fnAttr = (*this)->getAttrOfType<FlatSymbolRefAttr>("callee");
  if (!fnAttr)
    return emitOpError("requires a 'callee' symbol reference attribute");

  // Look up the callee - can be func.func or salt.func
  Operation *fn = symbolTable.lookupNearestSymbolFrom(*this, fnAttr);
  if (!fn)
    return emitOpError() << "'" << fnAttr.getValue()
                         << "' does not reference a valid function";

  return success();
}

//===----------------------------------------------------------------------===//
// TableGen Op Definitions
//===----------------------------------------------------------------------===//

#define GET_OP_CLASSES
#include "SaltOps.cpp.inc"
