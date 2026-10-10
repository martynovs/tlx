//! Pretty-print the surface AST of a `.tla` file.
//!
//! Usage: cargo run -p tlx-front --example parse -- [--debug | --cst] File.tla
//!   (default)  compact s-expression form
//!   --debug    full `{:#?}` dump (with spans)
//!   --cst      raw tree-sitter CST

use tlx_front::ast::FileId;
use tlx_front::parse::{parse_cst, parse_module, sexp};

fn main() {
    let mut mode = "sexp";
    let mut path = None;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--debug" => mode = "debug",
            "--cst" => mode = "cst",
            _ => path = Some(a),
        }
    }
    let Some(path) = path else {
        eprintln!("usage: parse [--debug | --cst] File.tla");
        std::process::exit(2);
    };
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(2);
    });
    if mode == "cst" {
        println!("{}", parse_cst(&src).expect("parse").root_node().to_sexp());
        return;
    }
    match parse_module(&src, FileId(0)) {
        Ok(m) if mode == "debug" => println!("{m:#?}"),
        Ok(m) => println!("{}", sexp::module(&m)),
        Err(diags) => {
            for d in diags {
                eprintln!("{path}:{d}");
            }
            std::process::exit(1);
        }
    }
}
