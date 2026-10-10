//! The order in which TLC prints the variables of a state.
//!
//! TLC takes the state variables from `ModuleNode.getVariableDecls()` of the
//! root module: the variable declarations found by enumerating SANY's symbol
//! table of the module (a `java.util.Hashtable` keyed by the symbol names),
//! reversed. That order depends on the hash codes of the variable names, on
//! the table's capacity (which grows with the number of symbols in the
//! table) and on the insertion order of the symbols. This module rebuilds
//! the root module's symbol table as SANY does (`Context`: the built-in
//! operators, then the symbols of each EXTENDS, then the module's own units)
//! and replays the Hashtable's insertions for the variables.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tlx_front::ast::{self, FileId, Unit};
use tlx_sema::builtins::{COMMUNITY_MODULES, STD_MODULES};
use tlx_sema::Program;

/// The number of SANY's built-in operators (`Context.initialContext`),
/// which every module's symbol table starts with.
const BUILTINS: usize = 72;

/// A symbol table key: an operator / declaration name, or a module name
/// (SANY keys modules by a distinct `ModuleName`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Key {
    Sym(Arc<str>),
    Module(Arc<str>),
}

/// The kind of symbol an entry is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Constant,
    Variable,
    /// Operator / function definitions (also `I!X` of named instances).
    Def,
    /// Named theorems and assumptions.
    Theorem,
    /// The name `I` of `I == INSTANCE M`.
    Instance,
    Module,
}

#[derive(Clone, Debug)]
struct Entry {
    key: Key,
    kind: Kind,
    local: bool,
}

/// A module's symbol table (without the built-ins), in insertion order.
#[derive(Default)]
struct Table {
    entries: Vec<Entry>,
    keys: HashSet<Key>,
}

impl Table {
    fn add(&mut self, key: Key, kind: Kind, local: bool) {
        if self.keys.insert(key.clone()) {
            self.entries.push(Entry { key, kind, local });
        }
    }
}

/// A parsed module, with the names of its theorems (tlx's AST keeps only
/// those it can evaluate).
struct ModuleSrc {
    ast: ast::Module,
    theorems: Vec<Arc<str>>,
}

/// Finds and parses modules as tlx-sema does (the root module's directory,
/// the bundled standard modules and CommunityModules, then the library
/// paths), including modules tlx itself did not need to load.
struct Loader {
    dirs: Vec<PathBuf>,
    modules: HashMap<Arc<str>, Arc<ModuleSrc>>,
    files: u32,
}

impl Loader {
    fn add_source(&mut self, src: &str) -> Option<Vec<Arc<str>>> {
        let file = FileId(self.files);
        self.files += 1;
        let ms = tlx_front::parse::parse_modules(src, file).ok()?;
        let theorems = theorem_names(src);
        fn register(m: &ast::Module, theorems: &[(usize, Arc<str>)], out: &mut HashMap<Arc<str>, Arc<ModuleSrc>>) {
            let nested: Vec<(usize, usize)> = m
                .units
                .iter()
                .filter_map(|u| if let Unit::Module(i) = u { Some((i.span.start as usize, i.span.end as usize)) } else { None })
                .collect();
            let own = theorems
                .iter()
                .filter(|(at, _)| {
                    (m.span.start as usize..m.span.end as usize).contains(at) && !nested.iter().any(|(s, e)| (*s..*e).contains(at))
                })
                .map(|(_, n)| n.clone())
                .collect();
            out.entry(m.name.name.clone()).or_insert_with(|| Arc::new(ModuleSrc { ast: m.clone(), theorems: own }));
            for u in &m.units {
                if let Unit::Module(inner) = u {
                    register(inner, theorems, out);
                }
            }
        }
        for m in &ms {
            register(m, &theorems, &mut self.modules);
        }
        Some(ms.iter().map(|m| m.name.name.clone()).collect())
    }

    fn find(&mut self, name: &Arc<str>) -> Option<Arc<ModuleSrc>> {
        if let Some(m) = self.modules.get(name) {
            return Some(m.clone());
        }
        let file = format!("{name}.tla");
        let read = |dirs: &[PathBuf]| dirs.iter().find_map(|d| std::fs::read_to_string(d.join(&file)).ok());
        let (root_dir, libs) = self.dirs.split_at(self.dirs.len().min(1));
        let src = read(root_dir)
            .or_else(|| STD_MODULES.iter().find(|(n, _)| **n == **name).map(|(_, s)| s.to_string()))
            .or_else(|| COMMUNITY_MODULES.iter().find(|(n, _)| **n == **name).map(|(_, s)| s.to_string()))
            .or_else(|| read(libs))?;
        self.add_source(&src)?;
        self.modules.get(name).cloned()
    }
}

/// Builds symbol tables, memoized by module name.
struct Builder {
    loader: Loader,
    done: HashMap<Arc<str>, Arc<Vec<Entry>>>,
    /// Modules being built (guards against cycles).
    active: HashSet<Arc<str>>,
}

impl Builder {
    /// The symbol table of module `name` (None if unknown).
    fn table_of(&mut self, name: &Arc<str>) -> Option<Arc<Vec<Entry>>> {
        if let Some(t) = self.done.get(name) {
            return Some(t.clone());
        }
        let m = self.loader.find(name)?;
        if !self.active.insert(name.clone()) {
            return None;
        }
        let t = Arc::new(self.build(&m).entries);
        self.active.remove(name);
        self.done.insert(name.clone(), t.clone());
        Some(t)
    }

    fn build(&mut self, src: &ModuleSrc) -> Table {
        let m = &src.ast;
        let mut t = Table::default();
        // EXTENDS: the non-local symbols of each extended module.
        for e in &m.extends {
            if let Some(et) = self.table_of(&e.name) {
                for x in et.iter().filter(|x| !x.local) {
                    t.add(x.key.clone(), x.kind, false);
                }
            }
        }
        for u in &m.units {
            match u {
                Unit::Constants(cs) => cs.iter().for_each(|c| t.add(Key::Sym(c.name.name.clone()), Kind::Constant, false)),
                Unit::Variables(vs) => vs.iter().for_each(|v| t.add(Key::Sym(v.name.clone()), Kind::Variable, false)),
                Unit::OpDef(d) => t.add(Key::Sym(d.name.name.clone()), Kind::Def, d.local),
                Unit::FnDef(d) => t.add(Key::Sym(d.name.name.clone()), Kind::Def, d.local),
                Unit::Recursive(ds) => ds.iter().for_each(|d| t.add(Key::Sym(d.name.name.clone()), Kind::Def, false)),
                Unit::Assume { name: Some(n), .. } => t.add(Key::Sym(n.name.clone()), Kind::Theorem, false),
                Unit::Assume { name: None, .. } => {}
                Unit::Theorem { name, .. } => t.add(Key::Sym(name.name.clone()), Kind::Theorem, false),
                Unit::Module(inner) => t.add(Key::Module(inner.name.name.clone()), Kind::Module, false),
                Unit::Instance(i) => {
                    let it = self.table_of(&i.module.name).unwrap_or_default();
                    let public = it.iter().filter(|x| !x.local);
                    match &i.name {
                        // `I == INSTANCE M`: `I!X` for every definition,
                        // theorem and instance X of M, then I.
                        Some(n) => {
                            for x in public.filter(|x| matches!(x.kind, Kind::Def | Kind::Theorem | Kind::Instance)) {
                                if let Key::Sym(s) = &x.key {
                                    let kind = if x.kind == Kind::Theorem { Kind::Theorem } else { Kind::Def };
                                    t.add(Key::Sym(format!("{}!{s}", n.name).into()), kind, i.local);
                                }
                            }
                            t.add(Key::Sym(n.name.clone()), Kind::Instance, i.local);
                        }
                        // `INSTANCE M`: the definitions of M themselves.
                        None => public.filter(|x| x.kind == Kind::Def).for_each(|x| t.add(x.key.clone(), Kind::Def, i.local)),
                    }
                }
            }
        }
        // Theorems tlx does not keep (e.g. ASSUME/PROVE statements).
        for n in &src.theorems {
            t.add(Key::Sym(n.clone()), Kind::Theorem, false);
        }
        t
    }
}

/// The names of the theorems (`THEOREM Name == ...`, also LEMMA,
/// PROPOSITION and COROLLARY) in a source text, with their offsets.
fn theorem_names(src: &str) -> Vec<(usize, Arc<str>)> {
    let b = src.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    // Tokens: words (with their offsets) and `==`; comments and strings skipped.
    let mut toks: Vec<(usize, &str)> = Vec::new();
    let mut i = 0;
    let mut depth = 0;
    while i < b.len() {
        if b[i..].starts_with(b"(*") {
            depth += 1;
            i += 2;
        } else if depth > 0 {
            if b[i..].starts_with(b"*)") {
                depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
        } else if b[i..].starts_with(b"\\*") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if b[i..].starts_with(b"==") {
            toks.push((i, "=="));
            i += 2;
            while i < b.len() && b[i] == b'=' {
                i += 1;
            }
        } else if word(b[i]) {
            let s = i;
            while i < b.len() && word(b[i]) {
                i += 1;
            }
            toks.push((s, &src[s..i]));
        } else {
            if !b[i].is_ascii_whitespace() {
                toks.push((i, ""));
            }
            i += 1;
        }
    }
    toks.windows(3)
        .filter(|w| matches!(w[0].1, "THEOREM" | "LEMMA" | "PROPOSITION" | "COROLLARY") && w[2].1 == "==" && !w[1].1.is_empty())
        .map(|w| (w[1].0, Arc::from(w[1].1)))
        .collect()
}

/// Java's `String.hashCode`.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The variables (`(hash, position)`: their key's hash code and their
/// position in the insertion order of a `java.util.Hashtable` filled with
/// `total` distinct keys) in the order of the table's enumeration, reversed.
fn hashtable_order(vars: &[(i32, usize)], total: usize) -> Vec<usize> {
    let mut cap = 11usize;
    let mut threshold = 8usize;
    // Buckets of the tracked (variable) keys only: the relative order of two
    // keys does not depend on the others, only the capacity does.
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); cap];
    let bucket = |h: i32, cap: usize| (h & 0x7FFF_FFFF) as usize % cap;
    let mut next_var = 0;
    for count in 0..total {
        if count >= threshold {
            // Hashtable.rehash: old buckets from the last, each chain from
            // its head, each entry pushed onto the head of its new chain.
            let new_cap = cap * 2 + 1;
            let mut nb: Vec<Vec<usize>> = vec![Vec::new(); new_cap];
            for chain in buckets.iter().rev() {
                for &v in chain {
                    nb[bucket(vars[v].0, new_cap)].insert(0, v);
                }
            }
            buckets = nb;
            cap = new_cap;
            threshold = (cap as f64 * 0.75) as usize;
        }
        if next_var < vars.len() && vars[next_var].1 == count {
            buckets[bucket(vars[next_var].0, cap)].insert(0, next_var);
            next_var += 1;
        }
    }
    // Hashtable.elements(): buckets from the last, chains from their head;
    // ModuleNode.getVariableDecls reverses that.
    let mut order: Vec<usize> = buckets.iter().rev().flat_map(|c| c.iter().copied()).collect();
    order.reverse();
    order
}

/// The order in which TLC prints the variables of `prog`'s root module, as
/// indices into `prog.vars`; None if it cannot be determined.
pub fn tlc_var_order(prog: &Program) -> Option<Vec<usize>> {
    let mut loader = Loader { dirs: prog.search_paths().to_vec(), modules: HashMap::new(), files: 0 };
    let root = loader.add_source(&prog.files.first()?.src)?.first()?.clone();
    let mut b = Builder { loader, done: HashMap::new(), active: HashSet::new() };
    let m = b.loader.find(&root)?;
    let table = b.build(&m);
    let vars: Vec<(i32, usize, Arc<str>)> = table
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == Kind::Variable)
        .map(|(i, e)| match &e.key {
            Key::Sym(s) | Key::Module(s) => (java_hash(s), BUILTINS + i, s.clone()),
        })
        .collect();
    if vars.len() != prog.vars.len() {
        return None;
    }
    let hashed: Vec<(i32, usize)> = vars.iter().map(|(h, p, _)| (*h, *p)).collect();
    let order = hashtable_order(&hashed, BUILTINS + table.entries.len());
    order.into_iter().map(|v| prog.vars.iter().position(|pv| *pv.name == *vars[v].2)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_string_hash() {
        assert_eq!(java_hash("done"), 3089282);
        assert_eq!(java_hash("zz"), 3904);
    }

    #[test]
    fn hashtable_enumeration() {
        // VARIABLES s, done, b, zz, a after 72 built-ins and Naturals' 12
        // symbols, in a table of 95 symbols: TLC prints done, zz, a, b, s.
        let names = ["s", "done", "b", "zz", "a"];
        let vars: Vec<(i32, usize)> = names.iter().enumerate().map(|(i, n)| (java_hash(n), 84 + i)).collect();
        let order: Vec<&str> = hashtable_order(&vars, 95).into_iter().map(|i| names[i]).collect();
        assert_eq!(order, ["done", "zz", "a", "b", "s"]);
    }

    #[test]
    fn theorems_in_text() {
        let src = "THEOREM T1 == TRUE\n(* LEMMA L0 == x *) LEMMA L1 == ASSUME NEW x PROVE x \\* COROLLARY C == 1\nTHEOREM TRUE";
        let names: Vec<String> = theorem_names(src).into_iter().map(|(_, n)| n.to_string()).collect();
        assert_eq!(names, ["T1", "L1"]);
    }
}
