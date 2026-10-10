//! The native plan of a model: which functions are native, which of them
//! call host regions (mixed mode, docs/mixed-mode.md §4), and where the
//! regions are called from. Both backends follow one plan, so that their
//! libraries are interchangeable (same functions, same region table).
//!
//! The C generator (`cgen`) is the reference backend: it makes the plan
//! while generating (`RegionBuilder`; an expression C cannot compile
//! becomes a region) and returns it with its source (`cgen::Library`).
//! The JIT (`jit`) consumes it: it calls the plan's regions at their call
//! sites (`RegionCursor`) and must reach every one of them exactly once,
//! and its `NativeSet` must equal the plan's (`enc::native`).
//!
//! The decisions shared by the backends live here: which functions may
//! call regions at all (`RegionOracle::mixes_action`/`mixes_pred`), the
//! identity of a call site (`site`), and the mixed-mode settings (`Mixed`).

// Only the JIT backend calls the regions of a plan, so without the `jit`
// feature `RegionCursor` (and its collections) is dead code.
#[cfg(any(feature = "jit", test))]
use std::collections::{HashMap, VecDeque};

use crate::enc::mixed::{Reads, Region, RegionTable};
use crate::ir::*;
use crate::lower::Lowerer;
use crate::types::Typing;

/// Which functions of a generated library are native (the others return
/// `TLX_UNSUPPORTED`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeSet {
    pub init: bool,
    pub actions: Vec<bool>,
    pub invariants: Vec<bool>,
    pub action_properties: Vec<bool>,
    /// Number of native functions calling host regions.
    pub mixed: usize,
    /// Per function: native and calling host regions.
    pub mixed_init: bool,
    pub mixed_actions: Vec<bool>,
    pub mixed_invariants: Vec<bool>,
    pub mixed_action_properties: Vec<bool>,
}

impl NativeSet {
    /// Whether anything at all is native.
    pub fn any(&self) -> bool {
        self.init || [&self.actions, &self.invariants, &self.action_properties].iter().any(|v| v.contains(&true))
    }
}

/// The plan of a library: its native functions, the host regions they
/// call (empty without mixed mode) and, per region, its call site.
pub struct NativePlan {
    pub native: NativeSet,
    pub regions: RegionTable,
    /// Per region: where it is called from (`site`), for the other backend
    /// to make the same calls.
    pub sites: Vec<usize>,
}

impl NativePlan {
    /// The regions of the plan, for a backend to call (`RegionCursor`).
    pub fn region_plan<'t>(&'t self, oracle: &'t dyn RegionOracle) -> RegionPlan<'t> {
        RegionPlan { oracle, table: &self.regions, sites: &self.sites }
    }
}

/// The call site of a region: the address of the IR node it evaluates
/// (valid while the generated functions are), with the low bit set for
/// the encoding of the node's value in another representation
/// (`CGen::encoded`).
pub fn site(e: &Expr, convert: bool) -> usize {
    e as *const Expr as usize | convert as usize
}

/// What code generation needs to know about regions beyond their IR.
pub trait RegionOracle {
    /// The state variables region `e` reads (None: unknown).
    fn reads(&self, e: &Expr) -> Option<Reads>;
    /// Whether running `s` / evaluating `e` may evaluate a side-effecting
    /// or nondeterministic builtin (R4: such a function is not mixed, as a
    /// failed run is redone by the interpreter).
    fn stmt_volatile(&self, s: &Stmt) -> bool;
    fn expr_volatile(&self, e: &Expr) -> bool;
    /// `stmt_volatile`, not counting output (Print/PrintT): the host holds
    /// an action's output back until its native run succeeds.
    fn stmt_volatile_beyond_output(&self, s: &Stmt) -> bool {
        self.stmt_volatile(s)
    }

    /// Whether Init `f` may call regions.
    fn mixes_action(&self, f: &ActionFn) -> bool {
        !self.stmt_volatile(&f.body)
    }

    /// Whether an action `f` (not Init) may call regions: its regions may
    /// print (the host holds the output back until the run succeeds,
    /// `EncModel::run_action`).
    fn mixes_next(&self, f: &ActionFn) -> bool {
        !self.stmt_volatile_beyond_output(&f.body)
    }

    /// Whether a predicate `p` may call regions.
    fn mixes_pred(&self, p: &PredFn) -> bool {
        !self.expr_volatile(&p.body)
    }
}

impl RegionOracle for Lowerer<'_> {
    fn reads(&self, e: &Expr) -> Option<Reads> {
        self.expression_summary(e).reads
    }
    fn stmt_volatile(&self, s: &Stmt) -> bool {
        Lowerer::stmt_volatile(self, s)
    }
    fn expr_volatile(&self, e: &Expr) -> bool {
        self.expression_summary(e).effects.volatile
    }
    fn stmt_volatile_beyond_output(&self, s: &Stmt) -> bool {
        Lowerer::stmt_volatile_beyond_output(self, s)
    }
}

/// Mixed-mode generation settings.
pub struct Mixed<'a> {
    pub oracle: &'a dyn RegionOracle,
    /// Testing (`TLX_MIXED_FORCE=k`): also make every k-th expression C
    /// could compile a region (0: none), to exercise the region calls on
    /// specs that are natively compilable.
    pub force: u32,
    /// Also mix functions with regions in statement loops that depend on
    /// the element (see `CGen::check_loop_regions`; the compiled tier always does).
    pub loops: bool,
}

/// The regions of a plan being made, with their call sites (kept in
/// step: region `i` is called from `sites[i]`).
#[derive(Default)]
pub(crate) struct RegionBuilder {
    regions: Vec<Region>,
    sites: Vec<usize>,
}

impl RegionBuilder {
    pub(crate) fn len(&self) -> usize {
        self.regions.len()
    }

    /// Add a region called from `site`; returns its id.
    pub(crate) fn push(&mut self, region: Region, site: usize) -> usize {
        self.regions.push(region);
        self.sites.push(site);
        self.regions.len() - 1
    }

    /// Drop the regions made since there were `len`.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.regions.truncate(len);
        self.sites.truncate(len);
    }

    /// The region table and the call sites.
    pub(crate) fn finish(&mut self, ty: &Typing) -> (RegionTable, Vec<usize>) {
        (RegionTable::new(ty, std::mem::take(&mut self.regions)), std::mem::take(&mut self.sites))
    }
}

/// The regions of a plan, as another backend calls them.
pub struct RegionPlan<'t> {
    /// Decides which functions may call regions (as for C).
    pub oracle: &'t dyn RegionOracle,
    pub table: &'t RegionTable,
    /// Per region: its call site (`NativePlan::sites`).
    pub sites: &'t [usize],
}

/// The regions of a plan not called yet, by call site: a backend takes
/// each region where it reaches its call site, and must take all of them.
#[cfg(any(feature = "jit", test))]
pub(crate) struct RegionCursor<'t> {
    #[cfg(feature = "jit")]
    pub(crate) oracle: &'t dyn RegionOracle,
    pub(crate) table: &'t RegionTable,
    /// The regions not called yet, by call site (in region id order).
    at: HashMap<usize, VecDeque<u32>>,
    /// Region calls made so far.
    calls: usize,
}

#[cfg(any(feature = "jit", test))]
impl<'t> RegionCursor<'t> {
    pub(crate) fn new(p: RegionPlan<'t>) -> RegionCursor<'t> {
        let mut at: HashMap<usize, VecDeque<u32>> = HashMap::new();
        for (rid, site) in p.sites.iter().enumerate() {
            at.entry(*site).or_default().push_back(rid as u32);
        }
        RegionCursor {
            #[cfg(feature = "jit")]
            oracle: p.oracle,
            table: p.table,
            at,
            calls: 0,
        }
    }

    /// Region calls made so far.
    pub(crate) fn calls(&self) -> usize {
        self.calls
    }

    /// Whether a region (still) has its call site at `site`.
    pub(crate) fn has(&self, site: usize) -> bool {
        self.at.get(&site).is_some_and(|q| !q.is_empty())
    }

    /// Whether a region (still) is called at `e` (either site of it).
    pub(crate) fn has_at(&self, e: &Expr) -> bool {
        self.has(site(e, false)) || self.has(site(e, true))
    }

    /// The next region called at `site` (`has`).
    pub(crate) fn take(&mut self, site: usize) -> u32 {
        self.calls += 1;
        self.at.get_mut(&site).and_then(VecDeque::pop_front).expect("a region at the site")
    }

    /// Err unless every region of the plan was called exactly once.
    pub(crate) fn finish(&self) -> Result<(), String> {
        let left: usize = self.at.values().map(VecDeque::len).sum();
        if left > 0 || self.calls != self.table.len() {
            return Err(format!("the JIT code calls {} of the {} host regions of the C code", self.calls, self.table.len()));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_plan_tests.rs"]
mod tests;
