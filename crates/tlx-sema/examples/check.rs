//! `cargo run --release -p tlx-sema --example check -- [--all] [-I dir]... File.tla...`
//!
//! Run tlx's parse and semantic checks (what `Program::load` reports) on
//! each file and print `OK <file>` or `ERR <file>: <first error>`
//! (used by scripts/sanydiff.py to compare with SANY).

use std::path::PathBuf;

use tlx_sema::Program;

fn main() {
    let mut includes = Vec::new();
    let mut files = Vec::new();
    let mut all = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--all" {
            all = true;
        } else if a == "-I" {
            includes.push(PathBuf::from(args.next().expect("-I needs a directory")));
        } else {
            files.push(PathBuf::from(a));
        }
    }
    for f in files {
        match Program::load(&f, &includes) {
            Ok(_) => println!("OK {}", f.display()),
            Err(es) => {
                let e = &es[0];
                println!("ERR {}: {} ({} error(s))", f.display(), e.to_string().replace('\n', " "), es.len());
                if all {
                    for e in &es[1..] {
                        println!("    {}", e.to_string().replace('\n', " "));
                    }
                }
            }
        }
    }
}
