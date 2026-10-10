//! tlx-compile: lowering HIR to the typed IR, the IR interpreter, C code
//! generation and the compiled `Model`s (design.md §6).

pub(crate) mod binding;
pub mod cgen;
pub mod coverage;
pub mod enc;
pub(crate) mod encoded_order;
pub mod exec;
pub mod inplace;
pub mod ir;
pub mod ir_fmt;
#[cfg(feature = "jit")]
pub mod jit;
pub mod lower;
pub mod native_plan;
pub(crate) mod paranoid;
pub(crate) mod rankmask;
pub mod settings;
pub mod types;
pub mod verify;
