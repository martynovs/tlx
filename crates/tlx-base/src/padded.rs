//! Values on cache lines of their own.

/// `T` aligned (and so sized) to 128 bytes: a value that one thread writes
/// often shares no cache line with values other threads use (128 rather
/// than 64: adjacent-line prefetchers fetch pairs of 64-byte lines).
#[repr(align(128))]
#[derive(Default, Debug)]
pub struct Padded<T>(pub T);

impl<T> std::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Padded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
