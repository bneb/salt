cmake_minimum_required(VERSION 3.20)
project(salt-opt)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

option(ENABLE_COVERAGE "Enable code coverage instrumentation" OFF)
if(ENABLE_COVERAGE)
    add_compile_options(-fprofile-instr-generate -fcoverage-mapping -stdlib=libc++)
    add_link_options(-fprofile-instr-generate -fcoverage-mapping -stdlib=libc++)
endif()

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    # add_compile_options(-fsanitize=address,undefined -fno-omit-frame-pointer)
    # add_link_options(-fsanitize=address,undefined)
endif()

# 1. Configuration
#
# SALT_LLVM_MAJOR pins the toolchain. find_package(MLIR REQUIRED CONFIG) with
# no version accepts whatever MLIR_DIR happens to point at, and that is how
# this build drifted: CONTRIBUTING.md documents LLVM 21, a stale cache in
# build/ was configured against llvm@18, and the salt-opt binary actually in
# use links llvm@21. Three answers to one question, none of them checked.
#
# The version skew is not cosmetic. salt-opt emits LLVM IR that a separate
# llc consumes downstream, so a mismatch shows up as an inexplicable codegen
# bug a long way from its cause. Fail at configure time instead.
set(SALT_LLVM_MAJOR 21 CACHE STRING "Required LLVM/MLIR major version")

find_package(MLIR REQUIRED CONFIG)
find_package(LLVM REQUIRED CONFIG)
find_package(Z3 REQUIRED)

if(NOT LLVM_VERSION_MAJOR VERSION_EQUAL SALT_LLVM_MAJOR)
  message(FATAL_ERROR
    "LLVM ${LLVM_VERSION_MAJOR} found, but this project requires LLVM "
    "${SALT_LLVM_MAJOR} (see CONTRIBUTING.md).\n"
    "  LLVM_DIR = ${LLVM_DIR}\n"
    "  MLIR_DIR = ${MLIR_DIR}\n"
    "Point both at the right install, e.g. on Homebrew:\n"
    "  -DLLVM_DIR=$(brew --prefix llvm@${SALT_LLVM_MAJOR})/lib/cmake/llvm\n"
    "  -DMLIR_DIR=$(brew --prefix llvm@${SALT_LLVM_MAJOR})/lib/cmake/mlir\n"
    "Or set -DSALT_LLVM_MAJOR=${LLVM_VERSION_MAJOR} if you intend to move the "
    "supported version, and update CONTRIBUTING.md to match.")
endif()

if(NOT MLIR_VERSION_MAJOR VERSION_EQUAL SALT_LLVM_MAJOR)
  message(FATAL_ERROR
    "MLIR ${MLIR_VERSION_MAJOR} does not match LLVM ${SALT_LLVM_MAJOR}. "
    "MLIR_DIR = ${MLIR_DIR}")
endif()

message(STATUS "salt-opt: LLVM/MLIR ${LLVM_VERSION_MAJOR} (pinned)")

list(APPEND CMAKE_MODULE_PATH "${MLIR_CMAKE_DIR}")
list(APPEND CMAKE_MODULE_PATH "${LLVM_CMAKE_DIR}")

include_directories(${MLIR_INCLUDE_DIRS} ${LLVM_INCLUDE_DIRS})
link_directories(${MLIR_LIBRARY_DIRS} ${LLVM_LIBRARY_DIRS})

include(TableGen)
include(AddLLVM)
include(AddMLIR)
include(HandleLLVMOptions)

add_definitions(${LLVM_DEFINITIONS})
include_directories(${Z3_INCLUDE_DIR} ${Z3_INCLUDE_DIRS} /opt/homebrew/include)
include_directories(${CMAKE_CURRENT_SOURCE_DIR}/src/dialect)
include_directories(${CMAKE_CURRENT_BINARY_DIR}/src/dialect)

# 2. Salt Dialect
add_subdirectory(src/dialect)

# 3. Salt Optimizer & Backend
add_executable(salt-opt 
  src/main.cpp 
  src/passes/Z3Verify.cpp
  src/passes/LowerSalt.cpp
)

target_include_directories(salt-opt PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/src/dialect
  ${CMAKE_CURRENT_BINARY_DIR}/src/dialect
)

# 4. Link Dependencies
target_link_libraries(salt-opt PRIVATE
  SaltDialect
  
  # --- MLIR Core ---
  MLIRIR MLIRSupport MLIRParser
  MLIRBytecodeOpInterface MLIRCallInterfaces MLIRSideEffectInterfaces
  
  # --- Standard Dialects & Transforms ---
  MLIRArithDialect MLIRFuncDialect MLIRSCFDialect MLIRMemRefDialect MLIRControlFlowDialect
  MLIRMathDialect    # Required for cttz
  MLIRVectorDialect  # Required for Vector ops
  MLIRLLVMDialect    # Required for LLVM Dialect
  
  # --- Optimization Pipeline (Linalg/Tensor) ---
  MLIRLinalgDialect
  MLIRLinalgTransforms       # createConvertLinalgToLoopsPass
  MLIRBufferizationDialect
  MLIRBufferizationTransforms # createOneShotBufferizePass
  MLIRAffineDialect
  MLIRTensorDialect
  MLIRReconcileUnrealizedCasts # FIX: Added back
  MLIRVectorTransforms         # FIX: Added back just in case
  MLIRTilingInterface          # <--- Generic Interface
  MLIRSCFTransforms            # <--- Tiling Implementation
  MLIRTensorToLinalg           # <--- createConvertTensorToLinalgPass
  MLIRMemRefTransforms         # <--- createExpandStridedMetadataPass
  
  # --- Conversions (The Bridges) ---
  MLIRArithToLLVM
  MLIRMathToLLVM
  MLIRMemRefToLLVM
  MLIRFuncToLLVM
  MLIRSCFToControlFlow
  MLIRControlFlowToLLVM
  MLIRVectorToLLVM           # Base conversion logic
  MLIRVectorToLLVMPass       # <--- FIX: This contains the actual Pass creation function
  
  # --- LLVM Translation & CodeGen ---
  MLIRLLVMToLLVMIRTranslation MLIRBuiltinToLLVMIRTranslation
  MLIROpenACCToLLVMIRTranslation
  MLIROpenMPToLLVMIRTranslation
  MLIRArmNeonToLLVMIRTranslation
  MLIRArmSMEToLLVMIRTranslation
  MLIRArmSVEToLLVMIRTranslation
  MLIRGPUToLLVMIRTranslation
  MLIRNVVMToLLVMIRTranslation
  MLIRROCDLToLLVMIRTranslation
  MLIRSPIRVToLLVMIRTranslation
  MLIROptLib
  
  # --- LLVM Backend Libraries ---
  LLVM
  
  # --- External ---
  ${Z3_LIBRARIES}
)
