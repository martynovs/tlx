//! `Statistics!ChiSquare(expected, actual, alpha)`, the Java override
//! `tlc2/overrides/Statistics.java`: both arguments are populated functions
//! of non-negative integers, `alpha` a string double in (0, 0.5], and the
//! result is `!(p < alpha)` for the p-value of Apache Commons Math 3.6.1's
//! `ChiSquareTest.chiSquareTest` — `1 - Gamma.regularizedGammaP((n-1)/2,
//! statistic/2)`.
//!
//! Commons Math's `regularizedGammaP`/`Q`, `ContinuedFraction.evaluate`,
//! `logGamma` (Lanczos) and the rescaling in `ChiSquareTest.chiSquare` are
//! reproduced literally, with the same `1e-14` epsilon.

use tlx_value::Value;

use crate::error::{EResult, EvalError};

const LANCZOS: [f64; 15] = [
    0.9999999999999971,
    57.15623566586292,
    -59.59796035547549,
    14.136097974741746,
    -0.4919138160976202,
    3.399464998481189e-5,
    4.652362892704858e-5,
    -9.837447530487956e-5,
    0.0001580887032249125,
    -0.00021026444172410488,
    0.00021743961811521265,
    -0.0001643181065367639,
    8.441822398385275e-5,
    -2.6190838401581408e-5,
    3.6899182659531625e-6,
];
const LANCZOS_G: f64 = 4.7421875;
const HALF_LOG_2_PI: f64 = 0.9189385332046727;
const EPSILON: f64 = 1e-14;
const SMALL: f64 = 1e-50;
const MAX_ITER: usize = i32::MAX as usize;

/// `Gamma.logGamma` (Lanczos, for x > 0).
fn log_gamma(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return f64::NAN;
    }
    let mut sum = 0.0;
    for i in (1..LANCZOS.len()).rev() {
        sum += LANCZOS[i] / (x + i as f64);
    }
    sum += LANCZOS[0];
    let tmp = x + LANCZOS_G + 0.5;
    (x + 0.5) * tmp.ln() - tmp + HALF_LOG_2_PI + (sum / x).ln()
}

/// `ContinuedFraction.evaluate(x, epsilon, maxIterations)` with the two
/// coefficient functions `getA`/`getB`.
fn continued_fraction(eps: f64, a_of: impl Fn(usize) -> f64, b_of: impl Fn(usize) -> f64) -> Result<f64, ()> {
    let mut h_prev = a_of(0);
    if h_prev.abs() <= SMALL {
        h_prev = SMALL;
    }
    let mut n = 1usize;
    let mut d_prev = 0.0;
    let mut c_prev = h_prev;
    let mut h_n = h_prev;
    while n < MAX_ITER {
        let a = a_of(n);
        let b = b_of(n);
        let mut d_n = a + b * d_prev;
        if d_n.abs() <= SMALL {
            d_n = SMALL;
        }
        let mut c_n = a + b / c_prev;
        if c_n.abs() <= SMALL {
            c_n = SMALL;
        }
        d_n = 1.0 / d_n;
        let delta_n = c_n * d_n;
        h_n = h_prev * delta_n;
        if h_n.is_infinite() || h_n.is_nan() {
            return Err(());
        }
        if (delta_n - 1.0).abs() < eps {
            break;
        }
        d_prev = d_n;
        c_prev = c_n;
        h_prev = h_n;
        n += 1;
    }
    if n >= MAX_ITER {
        return Err(());
    }
    Ok(h_n)
}

/// `Gamma.regularizedGammaP(a, x, epsilon, maxIterations)`.
fn regularized_gamma_p(a: f64, x: f64, eps: f64) -> Result<f64, ()> {
    if a.is_nan() || x.is_nan() || a <= 0.0 || x < 0.0 {
        return Ok(f64::NAN);
    }
    if x == 0.0 {
        return Ok(0.0);
    }
    if x >= a + 1.0 {
        return Ok(1.0 - regularized_gamma_q(a, x, eps)?);
    }
    let mut n = 0.0f64;
    let mut an = 1.0 / a;
    let mut sum = an;
    while (an / sum).abs() > eps && n < MAX_ITER as f64 && sum < f64::INFINITY {
        n += 1.0;
        an *= x / (a + n);
        sum += an;
    }
    if n >= MAX_ITER as f64 {
        return Err(());
    }
    if sum.is_infinite() {
        Ok(1.0)
    } else {
        Ok((-x + a * x.ln() - log_gamma(a)).exp() * sum)
    }
}

/// `Gamma.regularizedGammaQ(a, x, epsilon, maxIterations)`.
fn regularized_gamma_q(a: f64, x: f64, eps: f64) -> Result<f64, ()> {
    if a.is_nan() || x.is_nan() || a <= 0.0 || x < 0.0 {
        return Ok(f64::NAN);
    }
    if x == 0.0 {
        return Ok(1.0);
    }
    if x < a + 1.0 {
        return Ok(1.0 - regularized_gamma_p(a, x, eps)?);
    }
    let cf = continued_fraction(eps, |n| (2.0 * n as f64) + 1.0 - a + x, |n| n as f64 * (a - n as f64))?;
    Ok((-x + a * x.ln() - log_gamma(a)).exp() * (1.0 / cf))
}

/// `ChiSquareTest.chiSquare(expected, observed)`.
fn chi_square_statistic(expected: &[f64], observed: &[f64]) -> Result<f64, String> {
    if expected.len() < 2 {
        return Err(format!("ChiSquare: at least two categories are required, but the first argument has {}", expected.len()));
    }
    if expected.len() != observed.len() {
        return Err(format!(
            "ChiSquare: dimension mismatch: the first argument has {} values, the second {}",
            expected.len(),
            observed.len()
        ));
    }
    if let Some(e) = expected.iter().find(|e| **e <= 0.0) {
        return Err(format!("ChiSquare: expected counts must be positive, but one is {e}"));
    }
    if let Some(o) = observed.iter().find(|o| **o < 0.0) {
        return Err(format!("ChiSquare: observed counts must be non-negative, but one is {o}"));
    }
    let (mut sum_expected, mut sum_observed) = (0.0, 0.0);
    for i in 0..observed.len() {
        sum_expected += expected[i];
        sum_observed += observed[i];
    }
    let mut ratio = 1.0;
    let mut rescale = false;
    if (sum_expected - sum_observed).abs() > 1e-5 {
        ratio = sum_observed / sum_expected;
        rescale = true;
    }
    let mut ssd = 0.0;
    for i in 0..observed.len() {
        if rescale {
            let dev = observed[i] - ratio * expected[i];
            ssd += dev * dev / (ratio * expected[i]);
        } else {
            let dev = observed[i] - expected[i];
            ssd += dev * dev / expected[i];
        }
    }
    Ok(ssd)
}

/// The p-value of `ChiSquareTest.chiSquareTest(expected, observed)`.
fn chi_square_p(expected: &[f64], observed: &[f64]) -> Result<f64, String> {
    let statistic = chi_square_statistic(expected, observed)?;
    let df = expected.len() as f64 - 1.0;
    // GammaDistribution.cumulativeProbability: 0 for x <= 0, else P(df/2, x/2).
    let cdf = if statistic <= 0.0 {
        0.0
    } else {
        regularized_gamma_p(df / 2.0, statistic / 2.0, EPSILON)
            .map_err(|()| "ChiSquare: the Chi-square p-value did not converge".to_string())?
    };
    Ok(1.0 - cdf)
}

/// The counts of a populated function (TLC: `normalize().toFcnRcd()` then
/// its values, so a sequence is in index order and a record/function in
/// stored order).
fn counts(v: &Value, which: &str) -> EResult<Vec<f64>> {
    let items: Vec<Value> = match v {
        Value::Tuple(elems) => elems.to_vec(),
        Value::Fcn(pairs, _) => pairs.iter().map(|(_, v)| v.clone()).collect(),
        Value::Record(fields) => fields.iter().map(|(_, v)| v.clone()).collect(),
        Value::LazyFcn(_) => return counts(&crate::eval::force_lazy_fcn(v.clone())?, which),
        _ => return Err(EvalError::new(format!("The {which} argument of ChiSquare should be a function, but instead it is:\n{v}"))),
    };
    items
        .iter()
        .map(|v| match v {
            Value::Int(i) => Ok(*i as f64),
            _ => Err(EvalError::new(format!(
                "The {which} argument of ChiSquare should be a function with integer values, but instead it is:\n{v}"
            ))),
        })
        .collect()
}

/// `ChiSquare(expected, actual, alpha)` (Statistics.java `chiSquare`).
pub fn chi_square(expected: &Value, actual: &Value, alpha: &Value) -> EResult<Value> {
    let Value::Str(s) = alpha else {
        return Err(EvalError::new(format!(
            "The third argument of ChiSquare should be a StringValue representing a double in (0, .5], but instead it is:\n{alpha}"
        )));
    };
    let alpha: f64 = s.as_str().trim().parse().map_err(|_| {
        EvalError::new(format!(
            "The third argument of ChiSquare should be a StringValue representing a double in (0, .5], but instead it is:\n{alpha}"
        ))
    })?;
    if !(alpha > 0.0 && alpha <= 0.5) {
        return Err(EvalError::new(format!("ChiSquare: the significance level must be in (0, 0.5], but it is {alpha}")));
    }
    let expected = counts(expected, "first")?;
    let actual = counts(actual, "second")?;
    let p = chi_square_p(&expected, &actual).map_err(EvalError::new)?;
    // TLC: `chiSquareTest(expected, observed, alpha) ? FALSE : TRUE`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN p-value must give TRUE, as in TLC
    Ok(Value::Bool(!(p < alpha)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values printed by Apache Commons Math 3.6.1 (the copy in
    /// `third_party/community/modules.jar`) through reflection. `FastMath`'s
    /// `log`/`exp` and the platform's differ by at most one ulp, so the
    /// gamma functions are compared to within a few ulp: an operator whose
    /// result is a threshold comparison is unaffected (the end-to-end
    /// agreement with TLC is checked on thousands of borderline cases).
    #[test]
    fn matches_commons_math() {
        let close = |got: f64, want: f64| assert!((got - want).abs() <= want.abs().max(1e-300) * 1e-14, "{got} != {want}");
        close(log_gamma(0.5), 0.572_364_942_924_700_1);
        close(log_gamma(100.5), 361.435_540_467_777_6);
        close(regularized_gamma_p(3.0, 7.0, EPSILON).unwrap(), 0.970_363_836_119_478_2);
        close(regularized_gamma_p(0.5, 0.5, EPSILON).unwrap(), 0.682_689_492_137_085_9);
        // chiSquareTest(exp, obs) with equal counts: statistic 0, p 1.
        assert_eq!(chi_square_p(&[6.0; 6], &[6.0; 6]).unwrap(), 1.0);
    }
}
