//! The iteration order of a Java `HashMap` keyed by TLC values, for natives
//! whose result TLC builds in that order (VectorClocks.java `causalOrder`).
//! TLC's `Value.hashCode` folds the value's FP64 fingerprint, whose
//! polynomial TLC picks per run (`-fp N`, random by default); this is the
//! order under `-fp 0` (`FP64.Polys[0]`), the polynomial tlx's TLC oracles
//! run with first (docs/compat.md D-25).

use tlx_value::Value;

/// `FP64.Polys[0]`.
const POLY0: u64 = 0x911498AE0E66BAD6;

/// TLC's value type codes (`tlc2.value.ValueConstants`).
const BOOLVALUE: u8 = 0;
const INTVALUE: u8 = 1;
const STRINGVALUE: u8 = 3;

/// `FP64.ByteModTable_7` for `POLY0` (`FP64.Init`).
fn mod_table() -> &'static [u64; 256] {
    static TABLE: std::sync::OnceLock<[u64; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        const ONE: u64 = 0x8000000000000000;
        let mut power = [0u64; 72];
        let mut t = ONE;
        for p in power.iter_mut() {
            *p = t;
            let mask = if t & 1 != 0 { POLY0 } else { 0 };
            t = (t >> 1) ^ mask;
        }
        let mut table = [0u64; 256];
        for (j, v) in table.iter_mut().enumerate() {
            for k in 0..8 {
                if j & (1 << k) != 0 {
                    *v ^= power[127 - 7 * 8 - k];
                }
            }
        }
        table
    })
}

/// `FP64.Extend` by one byte (also a `char`: only its low byte counts).
fn extend_byte(fp: u64, b: u32) -> u64 {
    (fp >> 8) ^ mod_table()[((b ^ fp as u32) & 0xFF) as usize]
}

/// `FP64.Extend(fp, int)`: its four bytes, low first.
fn extend_int(mut fp: u64, x: i32) -> u64 {
    let mut x = x as u32;
    for _ in 0..4 {
        fp = extend_byte(fp, x & 0xFF);
        x >>= 8;
    }
    fp
}

/// TLC's `Value.hashCode()` under `-fp 0`, for the values whose
/// fingerprint does not depend on the run (booleans, 32-bit integers,
/// strings); None for others.
pub(crate) fn tlc_hash_code(v: &Value) -> Option<i32> {
    let fp = match v {
        Value::Bool(b) => extend_byte(extend_byte(POLY0, BOOLVALUE as u32), if *b { 't' } else { 'f' } as u32),
        Value::Int(i) => extend_int(extend_byte(POLY0, INTVALUE as u32), i32::try_from(*i).ok()?),
        Value::Str(s) => {
            let units: Vec<u16> = s.as_str().encode_utf16().collect();
            let fp = extend_int(extend_byte(POLY0, STRINGVALUE as u32), units.len() as i32);
            units.iter().fold(fp, |fp, &c| extend_byte(fp, c as u32))
        }
        _ => return None,
    };
    Some(((fp >> 32) as i32) ^ (fp as i32))
}

/// The order in which a `java.util.HashMap` iterates distinct keys added
/// with `computeIfAbsent` in the order of `hashes` (their `hashCode`s), as
/// `causalOrder` builds its map: a new key goes to the front of its bucket
/// (`put` would append it); before adding a key to a map of more than 3/4
/// of its slots (16 at first) the table doubles, each bucket splitting in
/// two in order; a bucket that already has 7 keys makes a table of fewer
/// than 64 slots double too (`treeifyBin`). Iteration goes by bucket, then
/// along the bucket. Returns the insertion indices. (A bucket of 8 keys in
/// a table of 64 slots or more becomes a tree, whose order is not modeled:
/// it keeps the list order.)
pub(crate) fn hash_map_order(hashes: &[i32]) -> Vec<usize> {
    let spread = |h: i32| {
        let h = h as u32;
        (h ^ (h >> 16)) as usize
    };
    let mut table: Vec<Vec<usize>> = vec![Vec::new(); 16];
    let resize = |table: &mut Vec<Vec<usize>>| {
        let old = table.len();
        let mut next: Vec<Vec<usize>> = vec![Vec::new(); old * 2];
        for (j, bucket) in table.iter().enumerate() {
            for &k in bucket {
                next[if spread(hashes[k]) & old == 0 { j } else { j + old }].push(k);
            }
        }
        *table = next;
    };
    for (k, &h) in hashes.iter().enumerate() {
        if k > table.len() * 3 / 4 {
            resize(&mut table);
        }
        let n = table.len();
        let bucket = &mut table[spread(h) & (n - 1)];
        let before = bucket.len();
        bucket.insert(0, k);
        if before >= 7 && n < 64 {
            resize(&mut table);
        }
    }
    table.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tlx_value::Sym;

    /// The `hashCode()`s of TLC's values after `FP64.Init(0)`, and the
    /// order of a `HashMap` of them (printed by a Java program using
    /// tla2tools.jar).
    #[test]
    fn hash_codes_of_tlc() {
        let s = |x: &str| tlc_hash_code(&Value::Str(Sym::new(x))).unwrap();
        let i = |x: i64| tlc_hash_code(&Value::Int(x)).unwrap();
        assert_eq!([s("0"), s("1"), s("2"), s("3"), s("4")], [1464943983, -481173810, 1062236716, -1957631603, -2024592407]);
        assert_eq!([s("abc"), s("")], [556160539, 628812721]);
        assert_eq!([i(0), i(1), i(2), i(-5), i(1000000)], [-579344629, 1345083260, -943683612, 2037615272, 1285807310]);
        assert_eq!(tlc_hash_code(&Value::Bool(true)), Some(-1255538333));
        assert_eq!(tlc_hash_code(&Value::Bool(false)), Some(-569491157));
        assert!(tlc_hash_code(&Value::Int(1 << 40)).is_none());
        // `computeIfAbsent` of "4", "0", "3", "1", "2": Java iterates "4"
        // "2" "3" "0" "1".
        let keys = ["4", "0", "3", "1", "2"];
        let order = hash_map_order(&keys.map(s));
        assert_eq!(order.iter().map(|&k| keys[k]).collect::<Vec<_>>(), ["4", "2", "3", "0", "1"]);
    }

    /// Java's order of `computeIfAbsent` keys, with resizes (printed by a
    /// Java program using tla2tools.jar after `FP64.Init(0)`).
    #[test]
    fn hash_map_order_of_java() {
        let ints: Vec<i64> = (0..40).rev().collect();
        let hs: Vec<i32> = ints.iter().map(|&i| tlc_hash_code(&Value::Int(i)).unwrap()).collect();
        let got: Vec<i64> = hash_map_order(&hs).into_iter().map(|k| ints[k]).collect();
        let want = [
            38, 33, 3, 8, 30, 21, 4, 15, 25, 18, 1, 10, 28, 23, 6, 13, 27, 16, 36, 35, 5, 14, 24, 19, 2, 9, 31, 20, 32, 39, 34, 37, 7, 12,
            26, 17, 0, 11, 29, 22,
        ];
        assert_eq!(got, want);
        let strs: Vec<String> = (0..20).map(|i| format!("n{i}")).collect();
        let hs: Vec<i32> = strs.iter().map(|x| tlc_hash_code(&Value::Str(Sym::new(x))).unwrap()).collect();
        let got: Vec<&str> = hash_map_order(&hs).into_iter().map(|k| strs[k].as_str()).collect();
        let want = [
            "n4", "n5", "n6", "n7", "n0", "n1", "n2", "n3", "n18", "n19", "n17", "n16", "n15", "n14", "n13", "n12", "n11", "n10", "n9",
            "n8",
        ];
        assert_eq!(got, want);
        let ints = [3, 1, 2, 7];
        let hs = ints.map(|i| tlc_hash_code(&Value::Int(i)).unwrap());
        assert_eq!(hash_map_order(&hs).into_iter().map(|k| ints[k]).collect::<Vec<_>>(), [1, 2, 7, 3]);
    }
}
