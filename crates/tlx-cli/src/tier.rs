//! The execution tiers the CLI reports on: the interpreter and the compiled
//! tier (`-compile`). What the end of a run (`report`, `live`) needs of a
//! model besides `Model` and its liveness atoms is the interpreted model
//! the specification was loaded into (the source of labels, ALIAS, the
//! POSTCONDITION, the temporal properties) and the values of the
//! variables of a state.

use std::sync::Arc;

use tlx_eval::model::liveness::AtomBits;
use tlx_eval::InterpModel;
use tlx_runtime::Model;
use tlx_value::Value;

pub(crate) trait Tier: Model + AtomBits<<Self as Model>::State> {
    /// The interpreted model (the tier itself for the interpreter).
    fn base(&self) -> &InterpModel;

    /// The values of the variables of state `s`.
    fn values(&self, s: &Self::State) -> Arc<[Value]>;

    /// The level of state `s` (`TLCGet("level")`).
    fn level(&self, s: &Self::State) -> u32;
}

impl Tier for InterpModel {
    fn base(&self) -> &InterpModel {
        self
    }

    fn values(&self, s: &Self::State) -> Arc<[Value]> {
        s.vals.clone()
    }

    fn level(&self, s: &Self::State) -> u32 {
        s.level
    }
}

impl Tier for tlx_compile::enc::EncModel {
    fn base(&self) -> &InterpModel {
        &self.base
    }

    fn values(&self, s: &Self::State) -> Arc<[Value]> {
        tlx_compile::enc::EncModel::values(self, s)
    }

    fn level(&self, s: &Self::State) -> u32 {
        s.level
    }
}
