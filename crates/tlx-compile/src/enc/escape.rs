//! Experimental action-local deoptimization over an immutable external layout.
//! Versions are transient execution codecs, never queue/checkpoint codecs.
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tlx_eval::InterpModel;
use tlx_layout::escape::EscapeLayout;
use tlx_layout::{Layout, Shape};
use tlx_runtime::stats;
use tlx_value::Value;

use super::model::EncModel;
use crate::settings::Settings;

const MAX_VERSIONS: usize = 3;

struct Versions {
    shapes: Vec<Shape>,
    builds: usize,
    ready: Vec<Arc<EncModel>>,
    pending: Option<JoinHandle<Arc<EncModel>>>,
}

pub(super) struct Escape {
    pub codec: EscapeLayout,
    /// File/register/random effects cannot be rolled back per action.
    pub irreversible_actions: Vec<bool>,
    settings: Settings,
    base: Arc<InterpModel>,
    versions: Mutex<Versions>,
    pub deopts: tlx_base::counter::Striped,
    pub guards: tlx_base::counter::Striped,
}

impl Escape {
    pub fn new(layout: Arc<Layout>, base: Arc<InterpModel>, settings: &Settings, irreversible_actions: Vec<bool>) -> Self {
        let mut settings = settings.clone();
        settings.escape = false;
        let shapes = layout.vars.clone();
        Self {
            codec: EscapeLayout::new(layout),
            irreversible_actions,
            settings,
            base,
            versions: Mutex::new(Versions { shapes, builds: 0, ready: Vec::new(), pending: None }),
            deopts: tlx_base::counter::Striped::new(),
            guards: tlx_base::counter::Striped::new(),
        }
    }

    fn collect(v: &mut Versions) {
        if v.pending.as_ref().is_some_and(|j| j.is_finished()) {
            v.ready.push(v.pending.take().unwrap().join().expect("escape compiler thread panicked"));
        }
    }

    pub fn version(&self) -> Option<Arc<EncModel>> {
        let mut v = self.versions.lock().unwrap();
        Self::collect(&mut v);
        v.ready.last().cloned()
    }

    /// Record all observed output slots, batching misses while one compile is
    /// in flight. At most two specialized versions and one all-Dyn version.
    pub fn observe(&self, vals: &[Value]) {
        let mut v = self.versions.lock().unwrap();
        Self::collect(&mut v);
        for (s, value) in v.shapes.iter_mut().zip(vals) {
            if !s.contains(value) {
                *s = tlx_layout::infer::widen(s, value);
            }
        }
        if v.pending.is_some() || v.builds == MAX_VERSIONS {
            return;
        }
        if v.ready.last().is_some_and(|m| m.layout.vars == v.shapes) {
            return;
        }
        v.builds += 1;
        let shapes = if v.builds == MAX_VERSIONS { vec![Shape::Dyn; vals.len()] } else { v.shapes.clone() };
        v.shapes = shapes.clone();
        let base = self.base.clone();
        let settings = self.settings.clone();
        v.pending = Some(std::thread::spawn(move || Arc::new(EncModel::new(base, Arc::new(Layout::new(shapes)), Vec::new(), &settings))));
    }

    pub fn models(&self) -> Vec<Arc<EncModel>> {
        let mut v = self.versions.lock().unwrap();
        Self::collect(&mut v);
        v.ready.clone()
    }

    pub fn report(&self) {
        let mut v = self.versions.lock().unwrap();
        Self::collect(&mut v);
        stats::set("escape irreversible actions", stats::Metric::Count(self.irreversible_actions.iter().filter(|&&v| v).count() as u64));
        stats::set("escape deoptimizations", stats::Metric::Count(self.deopts.get()));
        stats::set("escape guard failures", stats::Metric::Count(self.guards.get()));
        stats::set("escape recompilations", stats::Metric::Count(v.builds as u64));
        stats::set("escape generic version requested", stats::Metric::Count(u64::from(v.builds == MAX_VERSIONS)));
        stats::set("escape compiler pending", stats::Metric::Count(u64::from(v.pending.is_some())));
        stats::set("escape states reencoded", stats::Metric::Count(0));
        if let Some(s) = self.codec.store_stats() {
            stats::set("memory: escape store nodes and payloads", stats::Metric::Bytes(s.node_bytes));
            stats::set("memory: escape store index (est.)", stats::Metric::Bytes(s.index_bytes));
        }
    }
}

impl Drop for Escape {
    fn drop(&mut self) {
        if let Some(j) = self.versions.get_mut().unwrap().pending.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_shapes_publish_a_generic_version_after_three_requests() {
        let src = "---- MODULE EscapeStorm ----\nEXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == UNCHANGED x\n====";
        let base = Arc::new(tlx_testkit::spec::load(src, "INIT Init\nNEXT Next\n"));
        let settings = Settings::from_lookup(|key| (key == "TLX_NO_NATIVE").then(|| "1".into())).unwrap();
        let escape = Escape::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 1 }])), base, &settings, Vec::new());
        for value in [Value::Int(2), Value::Int(1000), Value::str("new shape")] {
            escape.observe(&[value]);
            // Synchronize publication directly: the storm bound must not
            // depend on a fast or slow compiler thread in this unit test.
            let pending = escape.versions.lock().unwrap().pending.take().unwrap();
            let model = pending.join().unwrap();
            escape.versions.lock().unwrap().ready.push(model);
        }
        for value in [Value::Int(i64::MAX), Value::tuple(vec![Value::Int(42)])] {
            escape.observe(&[value]);
        }
        let versions = escape.versions.lock().unwrap();
        assert_eq!(versions.builds, MAX_VERSIONS);
        assert_eq!(versions.ready.len(), MAX_VERSIONS);
        assert_eq!(versions.shapes, vec![Shape::Dyn]);
        assert_eq!(versions.ready.last().unwrap().layout.vars, vec![Shape::Dyn]);
        assert!(versions.pending.is_none());
    }
}
