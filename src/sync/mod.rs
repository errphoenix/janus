use std::{
    ops::Deref,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncError {
    TimeoutExceeded { exceed_time_ns: u128 },
    Locked,
}

pub type SyncResult = Result<(), SyncError>;

#[derive(Debug, Clone)]
pub struct Mirror<T: Clone> {
    local: T,
    version: usize,

    ptr: Arc<*mut T>,
    latest: Arc<AtomicUsize>,

    /// Indicates whether the underlying data is currently being read or
    /// written to.
    rw_signal: Arc<AtomicBool>,
}

impl<T: Default + Clone> Default for Mirror<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone> Mirror<T> {
    pub fn new(value: T) -> Self {
        let local = value.clone();
        let latest = Arc::new(AtomicUsize::new(0));
        let ptr = Arc::new(Box::into_raw(Box::new(value)));
        let rw_signal = Arc::new(AtomicBool::new(false));

        Self {
            local,
            version: 0,

            ptr,
            latest,
            rw_signal,
        }
    }

    /// Publish a new `value` to the shared data.
    ///
    /// This operation blocks if the data is currently being synchronised by
    /// other [`Mirror`] instances.
    ///
    /// Nonetheless, synchronisation is a very small operation; thus you can
    /// expect the block to be very short in most cases.
    ///
    /// # Notes on Synchronisation
    ///
    /// After this operation, any other [`Mirror`] instances will require a
    /// synchronisation or else they will keep pointing to their local stale
    /// data.
    ///
    /// The specific instance of [`Mirror`] that has published a new `value`
    /// does not require any synchronisation.
    /// This is important to keep in mind, especially in the case of
    /// single-producer scenarios: the producer will never need a
    /// synchronisation.
    pub fn publish(&mut self, value: T) {
        while self
            .rw_signal
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            std::thread::yield_now();
        }

        // SAFETY: we ensure the underlying pointer is unused by
        //         spinlocking for the state of the shared rw_signal.
        //         At the same time, we lock the signal again to avoid
        //         writes or other sync operations during our operation.
        unsafe {
            std::ptr::copy_nonoverlapping(&value as *const T, *self.ptr, 1);
        }

        self.rw_signal.store(false, Ordering::Release);
        self.version = self.latest.fetch_add(1, Ordering::Release) + 1;
        self.local = value;
    }

    /// Checks whether the [`Mirror`] is out of sync.
    pub fn check_sync_status(&self) -> bool {
        let latest_version = self.latest.load(Ordering::Acquire);
        self.version == latest_version
    }

    /// Attempt to synchronise without ever blocking.
    ///
    /// This will instantly give up if the rw signal is currently on, i.e. any
    /// other synchronisation operation is happening.
    ///
    /// Note that synchronisation locks are usually very short due to them
    /// being a very cheap operation, so this is usually not worth it unless
    /// synchronisation is really not crucial.
    ///
    /// In most cases, prefer the standard [`sync`](Mirror::sync).
    ///
    /// # Returns
    /// If the read/write lock is currently on, a [`SyncError::Locked`] is
    /// returned.
    /// Otherwise, [`Ok`] is returned.
    pub fn sync_noblock(&mut self) -> SyncResult {
        let latest_version = self.latest.load(Ordering::Acquire);
        if self.version < latest_version {
            if self
                .rw_signal
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(SyncError::Locked);
            }

            // SAFETY: we ensure the underlying pointer is unused by
            //         polling for the state of the shared rw_signal.
            //         At the same time, we lock the signal again to avoid
            //         writes or other sync operations during our operation.
            unsafe {
                std::ptr::copy_nonoverlapping(*self.ptr, &mut self.local, 1);
            }

            self.rw_signal.store(false, Ordering::Release);
            self.version = latest_version;
        }

        Ok(())
    }

    /// Synchronise the local cache with the real remote value.
    ///
    /// This will block if the rw signal is currently on, (i.e. any other
    /// synchronisation operation is happening) until it is unlocked.
    ///
    /// Note that synchronisation locks are usually very short due to them
    /// being a very cheap operation, so it usually does not incur heavy
    /// performance costs.
    ///
    /// This is a read operation during which the shared signal will be locked
    /// for its duration, forbidding other sync operations.
    ///
    /// # Returns
    /// This operation cannot fail. An [`Ok`] is always returned.
    pub fn sync(&mut self) -> SyncResult {
        let latest_version = self.latest.load(Ordering::Acquire);
        if self.version < latest_version {
            while self
                .rw_signal
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                std::thread::yield_now();
            }

            // SAFETY: we ensure the underlying pointer is unused by
            //         spinlocking for the state of the shared rw_signal.
            //         At the same time, we lock the signal again to avoid
            //         writes or other sync operations during our operation.
            unsafe {
                std::ptr::copy_nonoverlapping(*self.ptr, &mut self.local, 1);
            }

            self.rw_signal.store(false, Ordering::Release);
            self.version = latest_version;
        }
        Ok(())
    }

    /// Attempt to synchronise the local cache within a specified `timeout`.
    ///
    /// This will block if the rw signal is currently on, (i.e. any other
    /// synchronisation operation is happening) until it is unlocked or the
    /// timeout expires, in which case an error is returned..
    ///
    /// Note that synchronisation locks are usually very short due to them
    /// being a very cheap operation, so it usually does not incur heavy
    /// performance costs.
    ///
    /// This is a read operation during which the shared signal will be locked
    /// for its duration, forbidding other sync operations.
    ///
    /// # Returns
    /// If the read/write lock is not unlocked within the `timeout`, a
    /// [`SyncError::TimeoutExceeded`] containing the total waiting time (in
    /// nanos) is returned.
    /// Otherwise, [`Ok`] is returned.
    pub fn sync_timeout(&mut self, timeout: Duration) -> SyncResult {
        let start = Instant::now();
        let latest_version = self.latest.load(Ordering::Acquire);
        if self.version < latest_version {
            while self
                .rw_signal
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                let dt = Instant::now().duration_since(start);
                if dt > timeout {
                    return Err(SyncError::TimeoutExceeded {
                        exceed_time_ns: dt.as_nanos(),
                    });
                }
                std::thread::yield_now();
            }

            // SAFETY: we ensure the underlying pointer is unused by
            //         spinlocking for the state of the shared rw_signal.
            //         At the same time, we lock the signal again to avoid
            //         writes or other sync operations during our operation.
            unsafe {
                std::ptr::copy_nonoverlapping(*self.ptr, &mut self.local, 1);
            }

            self.rw_signal.store(false, Ordering::Release);
            self.version = latest_version;
        }
        Ok(())
    }

    /// Returns the local variable.
    ///
    /// This may not be synchronised.
    pub fn get(&self) -> &T {
        &self.local
    }
}

impl<T: Clone> Drop for Mirror<T> {
    fn drop(&mut self) {
        // only one left, we drop the data behind the shared pointer to
        // avoid memory leaks
        if Arc::strong_count(&self.ptr) == 1 {
            let _v = unsafe { Box::from_raw(*self.ptr) };
        }
    }
}

impl<T: Clone> Deref for Mirror<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.local
    }
}
