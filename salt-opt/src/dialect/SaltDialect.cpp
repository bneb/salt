//===- SaltDialect.cpp - Salt Dialect Implementation ---------------------===//
//
// This file implements the Salt dialect.
//
//===----------------------------------------------------------------------===//

#include "SaltDialect.h"
#include "SaltOps.h"
#include "mlir/IR/DialectImplementation.h"
#include "llvm/ADT/TypeSwitch.h"

using namespace mlir;
using namespace salt;

// Include the generated type declarations
#define GET_TYPEDEF_CLASSES
#include "SaltTypes.cpp.inc"

// Include TableGen-generated dialect definitions
#include "SaltDialect.cpp.inc"

//===----------------------------------------------------------------------===//
// Salt Dialect
//===----------------------------------------------------------------------===//

void SaltDialect::initialize() {
  addOperations<
#define GET_OP_LIST
#include "SaltOps.cpp.inc"
      >();

  // CRITICAL FIX: Register the types!
  addTypes<
#define GET_TYPEDEF_LIST
#include "SaltTypes.cpp.inc"
      >();
}
