//! Integer arithmetic with TLC's 32-bit overflow checks (tlc-semantics.md §4).
//! Arguments are expected to be within the i32 range (TLC `IntValue`s).

use crate::error::{err_of, ValueErrorKind as K};

const MIN: i64 = i32::MIN as i64;
const MAX: i64 = i32::MAX as i64;

#[inline]
fn check(r: i128, what: impl FnOnce() -> String) -> crate::Result<i64> {
    if r < MIN as i128 || r > MAX as i128 {
        return err_of(K::Overflow, format!("Overflow when computing {}", what()));
    }
    Ok(r as i64)
}

pub fn add(a: i64, b: i64) -> crate::Result<i64> {
    check(a as i128 + b as i128, || format!("{a}+{b}"))
}
pub fn sub(a: i64, b: i64) -> crate::Result<i64> {
    check(a as i128 - b as i128, || format!("{a}-{b}"))
}
pub fn mul(a: i64, b: i64) -> crate::Result<i64> {
    check(a as i128 * b as i128, || format!("{a}*{b}"))
}
/// `a ^ b`
pub fn pow(a: i64, b: i64) -> crate::Result<i64> {
    if b < 0 {
        return err_of(K::ArgumentError, format!("The second argument of ^ should be a natural number, but instead it is:\n{b}"));
    }
    if b == 0 {
        if a == 0 {
            return err_of(K::NullPowerNull, "0^0 is undefined.");
        }
        return Ok(1);
    }
    match a {
        0 | 1 => return Ok(a),
        -1 => return Ok(if b % 2 == 0 { 1 } else { -1 }),
        _ => {}
    }
    // |a| >= 2: intermediate magnitudes grow monotonically, so checking the
    // final result is equivalent to TLC's per-step check.
    let overflow = || err_of(K::Overflow, format!("Overflow when computing {a}^{b}"));
    let Ok(e) = u32::try_from(b) else { return overflow() };
    match (a as i128).checked_pow(e) {
        Some(r) if (MIN as i128..=MAX as i128).contains(&r) => Ok(r as i64),
        _ => overflow(),
    }
}
/// DyadicRationals' `GCD(n, m)` as TLC computes it. TLC does not evaluate the
/// TLA+ definition (`Max(Divisors(n) \cap Divisors(m))`, empty for arguments
/// below 1): it overrides `Reduce`, GCD's only (LOCAL) user, by the Java
/// method tlc2.overrides.DyadicRationals.reduce, which calls commons-math
/// `ArithmeticUtils.gcd(int, int)`: the non-negative gcd of |n| and |m|,
/// `gcd(0, 0) = 0` (Reduce then divides by zero, an error in both) and an
/// overflow error when the result is 2^31. With this GCD, Reduce's TLA+
/// definition yields what the Java method yields for every argument (Reduce
/// only receives `[num, den]` records built by Half and Add; the division is
/// exact, so `\div` and Java's truncating division agree).
pub fn gcd(a: i64, b: i64) -> crate::Result<i64> {
    let (mut x, mut y) = (a.unsigned_abs(), b.unsigned_abs());
    while y != 0 {
        (x, y) = (y, x % y);
    }
    if x > MAX as u64 {
        return err_of(K::Overflow, format!("overflow: gcd({a}, {b}) is 2^31"));
    }
    Ok(x as i64)
}
pub fn neg(a: i64) -> crate::Result<i64> {
    check(-(a as i128), || format!("-{a}"))
}
/// `a % b` (b must be positive; result in [0, b)).
pub fn modulo(a: i64, b: i64) -> crate::Result<i64> {
    if b <= 0 {
        return err_of(K::ArgumentError, format!("The second argument of % should be a positive number, but instead it is:\n{b}"));
    }
    Ok(a.rem_euclid(b))
}
/// `\div` as overridden by module Integers (floor division).
pub fn div_floor(a: i64, b: i64) -> crate::Result<i64> {
    if b == 0 {
        return err_of(K::DivisionByZero, "The second argument of \\div is 0.");
    }
    if a == MIN && b == -1 {
        return err_of(K::Overflow, format!("Overflow when computing {a} \\div {b}"));
    }
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        Ok(q - 1)
    } else {
        Ok(q)
    }
}
/// `\div` as overridden by module Naturals (TLC's quirky truncation, see notes §4):
/// `q = a / b` (truncating); `if (q < 0 && q * b != a) q--`. A truncated
/// quotient of 0 is never corrected, so `(0-1) \div 2 = 0`; `MIN \div -1`
/// wraps around to `MIN` (Java int semantics) instead of erroring.
pub fn div_naturals(a: i64, b: i64) -> crate::Result<i64> {
    if b == 0 {
        return err_of(K::DivisionByZero, "The second argument of \\div is 0.");
    }
    let (x, y) = (a as i32, b as i32);
    let mut q = x.wrapping_div(y);
    if q < 0 && q.wrapping_mul(y) != x {
        q -= 1;
    }
    Ok(q as i64)
}
