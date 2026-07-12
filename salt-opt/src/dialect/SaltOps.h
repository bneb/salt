#ifndef SALTOPS_H
#define SALTOPS_H

#include "SaltDialect.h"
#include "mlir/Bytecode/BytecodeImplementation.h"
#include "mlir/Bytecode/BytecodeOpInterface.h"
#include "mlir/IR/BuiltinTypes.h"
#include "mlir/IR/Dialect.h"
#include "mlir/IR/OpDefinition.h"
#include "mlir/IR/SymbolTable.h"
#include "mlir/Interfaces/CallInterfaces.h"
#include "mlir/Interfaces/SideEffectInterfaces.h"

#define GET_OP_CLASSES
// Include the generated type declarations
#define GET_TYPEDEF_CLASSES
#include "SaltTypes.h.inc"

#include "SaltOps.h.inc"

#endif // SALTOPS_H
