use std::{cell::UnsafeCell, ops::RangeBounds, slice::SliceIndex, sync::Arc};

#[derive(Debug, Clone)]
pub struct TriVec<T> {
    inner: Arc<UnsafeCell<[Vec<T>; 3]>>,
}
unsafe impl<T> std::marker::Sync for TriVec<T> {}
unsafe impl<T> std::marker::Send for TriVec<T> {}
impl<T: Clone> Default for TriVec<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Clone> TriVec<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UnsafeCell::new([Vec::new(), Vec::new(), Vec::new()])),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(UnsafeCell::new([
                Vec::with_capacity(capacity),
                Vec::with_capacity(capacity),
                Vec::with_capacity(capacity),
            ])),
        }
    }

    pub unsafe fn get_inner(&self, section: usize) -> &mut Vec<T> {
        let section = section % 3;
        let raw = unsafe { self.inner.get().as_mut_unchecked() };
        &mut raw[section]
    }

    pub fn reserve(&self, section: usize, additional: usize) {
        let inner = unsafe { self.get_inner(section) };
        inner.reserve(additional);
    }

    pub fn push(&self, section: usize, element: T) {
        let inner = unsafe { self.get_inner(section) };
        inner.push(element);
    }

    pub fn push_mut(&self, section: usize, element: T) -> &mut T {
        let inner = unsafe { self.get_inner(section) };
        inner.push_mut(element)
    }

    pub fn extend_from_slice(&self, section: usize, other: &[T]) {
        let inner = unsafe { self.get_inner(section) };
        inner.extend_from_slice(other);
    }

    pub fn get<I: SliceIndex<[T]>>(&self, section: usize, index: I) -> Option<&I::Output> {
        let inner = unsafe { self.get_inner(section) };
        inner.get(index)
    }

    pub fn get_mut<I: SliceIndex<[T]>>(&self, section: usize, index: I) -> Option<&mut I::Output> {
        let inner = unsafe { self.get_inner(section) };
        inner.get_mut(index)
    }

    pub fn remove(&self, section: usize, index: usize) -> Option<T> {
        let inner = unsafe { self.get_inner(section) };
        if index >= inner.len() {
            None
        } else {
            Some(inner.remove(index))
        }
    }

    pub fn iter(&self, section: usize) -> impl Iterator<Item = &T> {
        let inner = unsafe { self.get_inner(section) };
        inner.iter()
    }

    pub fn iter_mut(&self, section: usize) -> impl Iterator<Item = &mut T> {
        let inner = unsafe { self.get_inner(section) };
        inner.iter_mut()
    }

    pub fn drain(&self, section: usize, range: impl RangeBounds<usize>) -> std::vec::Drain<'_, T> {
        let inner = unsafe { self.get_inner(section) };
        inner.drain(range)
    }
}
