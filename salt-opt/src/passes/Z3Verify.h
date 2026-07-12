//===- Z3Verify.h - Z3 Verification Pass Declaration ------------*- C++ -*-===//
//
// Declares the Z3 verification pass for inter-procedural contract checking.
//
//===----------------------------------------------------------------------===//

#ifndef SALT_PASSES_Z3VERIFY_H
#define SALT_PASSES_Z3VERIFY_H

#include "mlir/Dialect/Func/IR/FuncOps.h"
#include "mlir/IR/BuiltinOps.h" // For ModuleOp
#include "mlir/IR/BuiltinTypes.h"
#include "mlir/Pass/Pass.h"
#include <memory>

namespace salt {

/// The Z3 verification pass.
/// Checks that all salt.call operations satisfy the callee's requires()
/// contract.
struct Z3VerifyPass
    : public mlir::PassWrapper<Z3VerifyPass,
                               mlir::OperationPass<mlir::ModuleOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(Z3VerifyPass)

  llvm::StringRef getArgument() const final { return "z3-verify"; }
  llvm::StringRef getDescription() const final {
    return "Verify Salt contracts using Z3 SMT solver";
  }

  struct FunctionSummary {
    mlir::func::FuncOp func;
    mlir::Operation *requiresOp = nullptr;
    mlir::Operation *ensuresOp = nullptr;
  };

  void runOnOperation() override;

  std::unique_ptr<mlir::Pass> clonePass() const override {
    return std::make_unique<Z3VerifyPass>();
  }
};

std::unique_ptr<mlir::Pass> createZ3VerifyPass();
void registerZ3VerifyPass();

} // namespace salt

#endif // SALT_PASSES_Z3VERIFY_H
