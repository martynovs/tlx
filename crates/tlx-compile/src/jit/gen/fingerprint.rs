//! Test-only IR twin of `tlx_fp_words` (tlx_abi.h).

use cranelift_codegen::ir::{InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::I64;

/// `tlx_fp_words(s, n)` (tlx_abi.h), unrolled for a fixed `n`.
pub(in crate::jit) fn fp_words_ir(b: &mut FunctionBuilder, s: Value, n: usize) -> Value {
    const K: i64 = 0x9e3779b97f4a7c15u64 as i64;
    fn mix(b: &mut FunctionBuilder, x: Value) -> Value {
        let t = b.ins().ushr_imm(x, 30);
        let x = b.ins().bxor(x, t);
        let x = b.ins().imul_imm(x, 0xbf58476d1ce4e5b9u64 as i64);
        let t = b.ins().ushr_imm(x, 27);
        let x = b.ins().bxor(x, t);
        let x = b.ins().imul_imm(x, 0x94d049bb133111ebu64 as i64);
        let t = b.ins().ushr_imm(x, 31);
        b.ins().bxor(x, t)
    }
    // h' = mix(h ^ w) + K * (i + 1)
    fn step(b: &mut FunctionBuilder, h: Value, w: Value, i: usize) -> Value {
        let x = b.ins().bxor(h, w);
        let m = mix(b, x);
        b.ins().iadd_imm(m, K.wrapping_mul(i as i64 + 1))
    }
    let load = |b: &mut FunctionBuilder, i: usize| b.ins().load(I64, MemFlags::trusted(), s, (i * 8) as i32);
    let mut h = b.ins().iconst(I64, K ^ n as i64);
    if n <= 8 {
        for i in 0..n {
            let w = load(b, i);
            h = step(b, h, w, i);
        }
        return mix(b, h);
    }
    let mut l: Vec<Value> = (0..8).map(|j| b.ins().iadd_imm(h, j as i64)).collect();
    for i in 0..n {
        let w = load(b, i);
        l[i & 7] = step(b, l[i & 7], w, i);
    }
    for (j, x) in l.into_iter().enumerate() {
        h = step(b, h, x, j);
    }
    mix(b, h)
}
