//! Unsigned varints and signed zigzag encoding used by persisted streams.

#[derive(Debug, PartialEq, Eq)]
pub enum VarintError {
    End,
    Overflow,
}

#[inline]
pub fn put_uvar(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[inline]
pub fn put_uvar_at(buf: &mut [std::mem::MaybeUninit<u8>], mut at: usize, mut value: u64) -> usize {
    while value >= 0x80 {
        buf[at].write(value as u8 | 0x80);
        at += 1;
        value >>= 7;
    }
    buf[at].write(value as u8);
    at + 1
}

#[inline]
fn read_uvar<const CHECKED: bool>(bytes: &[u8], at: &mut usize) -> Result<u64, VarintError> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = if CHECKED { *bytes.get(*at).ok_or(VarintError::End)? } else { bytes[*at] };
        *at += 1;
        if CHECKED && shift == 63 && byte > 1 {
            return Err(VarintError::Overflow);
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte < 0x80 {
            return Ok(value);
        }
        shift += 7;
    }
}

#[inline]
pub fn get_uvar(bytes: &[u8], at: &mut usize) -> Result<u64, VarintError> {
    read_uvar::<true>(bytes, at)
}

#[inline]
pub fn get_uvar_trusted(bytes: &[u8], at: &mut usize) -> u64 {
    read_uvar::<false>(bytes, at).unwrap()
}

#[inline]
pub fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

#[inline]
pub fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

    #[test]
    fn both_writers_and_readers_match_legacy_bytes() {
        for value in [0, 1, 127, 128, 255, 16383, 16384, u32::MAX as u64, 1 << 63, u64::MAX] {
            let mut expected = Vec::new();
            let mut remaining = value;
            while remaining >= 0x80 {
                expected.push(remaining as u8 | 0x80);
                remaining >>= 7;
            }
            expected.push(remaining as u8);
            let mut bytes = vec![42];
            put_uvar(&mut bytes, value);
            assert_eq!(&bytes[1..], expected);
            let mut spare = [MaybeUninit::new(0); 12];
            let end = put_uvar_at(&mut spare, 1, value);
            for (slot, expected) in spare[1..end].iter().zip(expected) {
                // SAFETY: all slots were initialized before the call.
                assert_eq!(unsafe { slot.assume_init() }, expected);
            }
            let mut checked = 1;
            let mut trusted = 1;
            assert_eq!(get_uvar(&bytes, &mut checked), Ok(value));
            assert_eq!(get_uvar_trusted(&bytes, &mut trusted), value);
            assert_eq!((checked, trusted), (end, end));
        }
    }

    #[test]
    fn corrupt_input_keeps_reader_offsets() {
        for (bytes, error, offset) in
            [(vec![], VarintError::End, 0), (vec![128], VarintError::End, 1), (vec![255; 10], VarintError::Overflow, 10)]
        {
            let mut at = 0;
            assert_eq!(get_uvar(&bytes, &mut at), Err(error));
            assert_eq!(at, offset);
        }
        let mut at = 0;
        assert_eq!(get_uvar(&[128, 0], &mut at), Ok(0));
        assert_eq!(at, 2);
    }

    #[test]
    fn signed_extremes() {
        for value in [i64::MIN, -128, -1, 0, 1, 128, i64::MAX] {
            assert_eq!(unzigzag(zigzag(value)), value);
        }
    }
}
