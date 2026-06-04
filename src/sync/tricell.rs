use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
pub struct TriCell<T>
where
    T: Default + Clone + Copy + Send + Sync,
{
    pointers: [UnsafeCell<T>; 3],
    write_i: AtomicUsize,
    read_i: AtomicUsize,
}

unsafe impl<T: Default + Clone + Copy + Sync + Send> Send for TriCell<T> {}

unsafe impl<T: Default + Clone + Copy + Sync + Send> Sync for TriCell<T> {}

pub type AdvanceResult = Result<usize, usize>;

impl<T> Default for TriCell<T>
where
    T: Default + Clone + Copy + Send + Sync,
{
    fn default() -> Self {
        Self {
            pointers: Default::default(),
            write_i: AtomicUsize::new(1),
            read_i: AtomicUsize::new(0),
        }
    }
}

impl<T> TriCell<T>
where
    T: Default + Clone + Copy + Send + Sync,
{
    pub fn new(value: T) -> Self {
        Self {
            pointers: [
                UnsafeCell::new(value),
                UnsafeCell::default(),
                UnsafeCell::default(),
            ],
            write_i: AtomicUsize::new(1),
            read_i: AtomicUsize::new(0),
        }
    }

    pub unsafe fn read_raw(&self) -> &UnsafeCell<T> {
        let read_i = self.read_i.load(Ordering::Relaxed);
        &self.pointers[read_i]
    }

    pub unsafe fn write_raw(&self) -> &UnsafeCell<T> {
        let write_i = self.write_i.load(Ordering::Relaxed);
        &self.pointers[write_i]
    }

    /// Advances the internal read and write indices of the triple buffer.
    ///
    /// # Returns
    /// The previous (before advancing) write and read index, respectively.
    ///
    /// These are `AdvanceResult` types, which will return `Ok` if the index
    /// has advanced, `Err` if it failed to advance. These contain the
    /// (previous, if `Ok`; current if `Err`) index value to use.
    pub fn advance(&self) -> (AdvanceResult, AdvanceResult) {
        let write = self
            .write_i
            .fetch_update(Ordering::Release, Ordering::Acquire, |i| Some((i + 1) % 3));
        let read = self
            .read_i
            .fetch_update(Ordering::Release, Ordering::Acquire, |i| Some((i + 1) % 3));

        (write, read)
    }

    /// Update the `value` stored in the current 'write' section and advance
    /// its index.
    ///
    /// If the index cannot be advanced, the value will not be updated.
    ///
    /// Note that [`TriCell`] requires an `advance` (index increase operation)
    /// **only once** per frame. Multiple `advance` in a frame may lead to
    /// unexpected behaviour.
    ///
    /// Consider [`TriCell::set`] for a set operation that does not update the
    /// internal index, which can be used in combiantion with
    /// [`TriCell::advance`].
    ///
    /// # Returns
    /// `Ok` with the previously stored value or `Err` if the index could not
    /// be updated.
    pub fn set_and_advance(&self, value: T) -> Result<T, ()> {
        if let (Ok(index), _) = self.advance() {
            let raw = self.pointers[index].get();
            let prev = unsafe { *raw };

            unsafe {
                *raw = value;
            }

            Ok(prev)
        } else {
            Err(())
        }
    }

    /// Update the `value` storede in the current `write` section.
    ///
    /// This operation does not advance the internal index. This must be done
    /// manually per-frame with [`TriCell::advance`], or consider using
    /// [`TriCell::set_and_advance`].
    pub fn set(&self, value: T) {
        unsafe { *self.write_raw().get() = value }
    }

    /// Get the stored value in the current section and advance the index.
    ///
    /// The stored value is returned even if the index fails to advance.
    pub fn get(&self) -> T {
        unsafe { *self.read_raw().get() }
    }
}
