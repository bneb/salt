//===- SaltDialect.td - Salt Dialect Definition ---------*- tablegen -*-===//
//
// Simplified Salt dialect for MVP Gauntlet verification.
// Includes !salt.region type, salt.call, and salt.verify.
//
//===----------------------------------------------------------------------===//

include "mlir/IR/OpBase.td"
include "mlir/Interfaces/SideEffectInterfaces.td"
include "mlir/IR/SymbolInterfaces.td"

//===----------------------------------------------------------------------===//
// Dialect Definition
//===----------------------------------------------------------------------===//

def Salt_Dialect : Dialect {
  let name = "salt";
  let cppNamespace = "::salt";
  let summary = "Salt: Verified Systems Programming Dialect";
  let description = [{
    The Salt dialect provides operations for verified systems programming,
    including contract verification (requires/ensures) and region-based memory.
  }];
  let useDefaultTypePrinterParser = 1;
}

//===----------------------------------------------------------------------===//
// Salt Types (REQUIRED for !salt.region)
//===----------------------------------------------------------------------===//

def RegionType : TypeDef<Salt_Dialect, "Region"> {
  let mnemonic = "region";
  let summary = "A handle to a memory arena";
  let description = [{
    A handle representing a memory region for region-based memory management.
  }];
}

//===----------------------------------------------------------------------===//
// Base Operation Class
//===----------------------------------------------------------------------===//

class Salt_Op<string mnemonic, list<Trait> traits = []> :
    Op<Salt_Dialect, mnemonic, traits>;

//===----------------------------------------------------------------------===//
// Verification Operations
//===----------------------------------------------------------------------===//

def VerifyOp : Salt_Op<"verify"> {
  let summary = "Contract verification assertion (requires/ensures)";
  let description = [{
    Represents a precondition (requires) or postcondition (ensures).
    This operation is consumed by the Z3 verification pass and erased
    before lowering to LLVM IR.
  }];
  let arguments = (ins StrAttr:$kind, I1:$condition);
  let assemblyFormat = "$kind `(` $condition `)` attr-dict";
}

//===----------------------------------------------------------------------===//
// Call Operation
//===----------------------------------------------------------------------===//

def CallOp : Salt_Op<"call", [
    DeclareOpInterfaceMethods<SymbolUserOpInterface>
  ]> {
  let summary = "Salt function call with implicit region forwarding";
  let description = [{
    Calls a function, forwarding the implicit region argument.
  }];
  let arguments = (ins FlatSymbolRefAttr:$callee, Variadic<AnyType>:$operands);
  let results = (outs Variadic<AnyType>:$results);
  let assemblyFormat = "$callee `(` $operands `)` attr-dict `:` functional-type($operands, $results)";
}

//===----------------------------------------------------------------------===//
// Error Handling
//===----------------------------------------------------------------------===//

def PanicOp : Salt_Op<"panic", [Terminator]> {
  let summary = "Abort execution with error message";
  let arguments = (ins StrAttr:$message);
  let assemblyFormat = "$message attr-dict";
}
