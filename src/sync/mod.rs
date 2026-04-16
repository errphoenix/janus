use std::{
    ops::Deref,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncError {
    TimeoutExceeded { exceed_time_ns: u128 },
    Locked,
}

pub type SyncResult = Result<(), SyncError>;

#[derive(Debug, Default, Clone)]
pub struct SequentialLock(Arc<AtomicU64>);

impl SequentialLock {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    pub fn lock(&self) {
        let mut seq = self.0.load(Ordering::Relaxed);
        loop {
            // even lock number = unlocked, else it's locked
            if seq % 2 == 0 {
                match self.0.compare_exchange_weak(
                    seq,
                    seq + 1,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(real) => seq = real,
                }
            } else {
                std::hint::spin_loop();
                seq = self.0.load(Ordering::Relaxed)
            }
        }
    }

    pub fn unlock(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Release) + 1
    }

    pub fn get(&self, ordering: Ordering) -> u64 {
        self.0.load(ordering)
    }
}

#[derive(Debug)]
pub struct Mirror<T: Clone> {
    local: T,
    version: u64,

    inner: NonNull<T>,
    seq_lock: SequentialLock,
    counter: Arc<AtomicUsize>,
}

impl<T: Default + Clone + Send + Sync> Default for Mirror<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone + Send + Sync> Clone for Mirror<T> {
    fn clone(&self) -> Self {
        self.counter.fetch_add(1, Ordering::Acquire);
        Self {
            local: self.local.clone(),
            version: self.version,
            inner: self.inner,
            seq_lock: self.seq_lock.clone(),
            counter: self.counter.clone(),
        }
    }
}

impl<T: Clone> Drop for Mirror<T> {
    fn drop(&mut self) {
        if self.counter.fetch_sub(1, Ordering::Release) == 1 {
            std::sync::atomic::fence(Ordering::Acquire);
            drop(unsafe { Box::from_raw(self.inner.as_ptr()) });
        }
    }
}

impl<T: Clone + Send + Sync> Mirror<T> {
    pub fn new(value: T) -> Self {
        let local = value.clone();
        let inner = Box::leak(Box::from(value));
        let seq_lock = SequentialLock::new();
        let counter = Arc::new(AtomicUsize::new(1));

        Self {
            local,
            version: 0,
            inner: NonNull::from(inner),
            seq_lock,
            counter,
        }
    }

    /// Mutate the inner value with an `operation ` and publish it.
    ///
    /// See [`Mirror::publish`].
    ///
    /// The `operation` is called once this mirror instance has ensured
    /// exclusive access to the underlying data.
    pub fn publish_with<F: FnOnce(&mut T)>(&mut self, operation: F) {
        self.seq_lock.lock();

        operation(&mut self.local);

        // SAFETY: we ensure the underlying pointer is unused by ensuring the
        // shared SequentialLock is on an EVEN value; which indicates it is
        // under no exclusive access and safe to write to.
        unsafe {
            std::ptr::copy_nonoverlapping(&self.local, self.inner.as_ptr(), 1);
        }

        self.version = self.seq_lock.unlock();
    }

    /// Publish a new `value` to the underlying shared pointer.
    ///
    /// This operation will block if the shared state is under exclusive
    /// access, ie. another Mirror is publishing to it.
    ///
    /// # Notes on Synchronisation
    ///
    /// After this operation, any other [`Mirror`] instances will require a
    /// synchronisation or else they will keep pointing to their local stale
    /// data.
    ///
    /// The specific instance of [`Mirror`] that has published a new `value`
    /// does not require any synchronisation.
    /// In the case of single-producer scenarios, the producer will never
    /// require an explicit [`Mirror::sync`] call.
    pub fn publish(&mut self, value: T) {
        self.seq_lock.lock();

        // SAFETY: we ensure the underlying pointer is unused by ensuring the
        // shared SequentialLock is on an EVEN value; which indicates it is
        // under no exclusive access and safe to write to.
        unsafe {
            std::ptr::copy_nonoverlapping(&value, self.inner.as_ptr(), 1);
        }

        self.local = value;
        self.version = self.seq_lock.unlock();
    }

    /// Checks whether the [`Mirror`] is up-to-date with the other accessors.
    pub fn check_sync_status(&self) -> bool {
        let seq = self.seq_lock.get(Ordering::Acquire);
        self.version == seq && seq % 2 == 0
    }

    /// Attempt to synchronise without ever blocking.
    ///
    /// This will instantly give up if shared state is under exclusive access.
    ///
    /// Note that synchronisation locks are usually very short due to them
    /// being a very cheap operation, so this is usually not worth it unless
    /// synchronisation is really not crucial or **very** frequent operations.
    ///
    /// In most cases, prefer the standard [`sync`](Mirror::sync).
    ///
    /// # Returns
    /// If the shared state is currently locked, a [`SyncError::Locked`] is
    /// returned.
    /// Otherwise, [`Ok`] is returned.
    pub fn sync_noblock(&mut self) -> SyncResult {
        loop {
            let seq_0 = self.seq_lock.get(Ordering::Acquire);

            if seq_0 % 2 != 0 {
                return Err(SyncError::Locked);
            }

            // up-to-date check
            if self.version == seq_0 {
                return Ok(());
            }

            // SAFETY: we ensure the underlying pointer is unused by ensuring the
            // shared SequentialLock is on an EVEN value; which indicates it is
            // under no exclusive access and safe to write to.
            unsafe {
                std::ptr::copy_nonoverlapping(self.inner.as_ptr(), &mut self.local, 1);
            }

            // ensure prior operations (copy from shared to local) is not
            // reordered before the next version load.
            std::sync::atomic::fence(Ordering::Acquire);
            let seq_1 = self.seq_lock.get(Ordering::Acquire);

            // value is guaranteed to not have changed since sync started.
            if seq_0 == seq_1 {
                self.version = seq_1;
                break;
            }

            // value changed while syncing: re-sync
            continue;
        }

        Ok(())
    }

    /// Synchronise the local cache with the real remote value.
    ///
    /// This will block if the shared state is under exclusive access, until it
    /// no longer is.
    ///
    /// Note that synchronisation locks are usually very short due to them
    /// being a very cheap operation, so it usually only incurs little, if any,
    /// performance costs.
    ///
    /// # Returns
    /// This operation cannot fail. An [`Ok`] is always returned.
    pub fn sync(&mut self) -> SyncResult {
        loop {
            let seq_0 = self.seq_lock.get(Ordering::Acquire);

            if seq_0 % 2 != 0 {
                std::hint::spin_loop();
                continue;
            }

            // up-to-date check
            if self.version == seq_0 {
                return Ok(());
            }

            // SAFETY: we ensure the underlying pointer is unused by ensuring the
            // shared SequentialLock is on an EVEN value; which indicates it is
            // under no exclusive access and safe to write to.
            unsafe {
                std::ptr::copy_nonoverlapping(self.inner.as_ptr(), &mut self.local, 1);
            }

            // ensure prior operations (copy from shared to local) is not
            // reordered before the next version load.
            std::sync::atomic::fence(Ordering::Acquire);
            let seq_1 = self.seq_lock.get(Ordering::Acquire);

            // value is guaranteed to not have changed since sync started.
            if seq_0 == seq_1 {
                self.version = seq_1;
                break;
            }

            // value changed while syncing: re-sync
            continue;
        }

        Ok(())
    }

    /// Returns the local variable.
    ///
    /// This does not directly ensure the value has been synchronised.
    ///
    /// You may want to do so by explicitly calling any of the sync functions.
    pub fn get(&self) -> &T {
        &self.local
    }
}

impl<T: Clone + Send + Sync> Deref for Mirror<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.local
    }
}

unsafe impl<T: Clone + Sync + Send> Sync for Mirror<T> {}
unsafe impl<T: Clone + Send + Sync> Send for Mirror<T> {}
