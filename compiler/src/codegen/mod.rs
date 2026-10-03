//! Native code generation from checked HIR and MIR.

mod abi;
mod layout;
mod llvm;

pub use llvm::emit;
