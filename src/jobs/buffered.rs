use std::cell::UnsafeCell;

use rayon::iter::{IntoParallelIterator, ParallelIterator};

#[derive(Clone, Debug, Default)]
pub struct WorkBuffers<T: Default, R: Default> {
    pub buffer: T,
    pub result: Vec<R>,
}

#[derive(Debug, Default)]
pub struct ThreadBuffers<T: Default, R: Default>(Vec<UnsafeCell<WorkBuffers<T, R>>>);

unsafe impl<T: Default, R: Default> Sync for ThreadBuffers<T, R> {}

impl<T: Default, R: Default> ThreadBuffers<T, R> {
    pub fn new(thread_count: usize) -> Self {
        let mut buffers = Vec::with_capacity(thread_count);

        for _ in 0..thread_count {
            buffers.push(UnsafeCell::new(WorkBuffers::default()));
        }

        Self(buffers)
    }

    pub fn buffers_raw(&self) -> &[UnsafeCell<WorkBuffers<T, R>>] {
        &self.0
    }

    pub fn buffers_mut(&mut self) -> impl Iterator<Item = &mut WorkBuffers<T, R>> {
        self.0.iter_mut().map(|cell| cell.get_mut())
    }

    pub fn get_current_mut(&self) -> &mut WorkBuffers<T, R> {
        let index = rayon::current_thread_index().expect("must be called within a rayon job");
        unsafe { &mut *self.0[index].get() }
    }
}

#[derive(Debug)]
pub struct BufferedRoutine<T, R>
where
    T: Default + Clone,
    R: Default + Clone,
{
    thread_buffers: ThreadBuffers<T, R>,
}

impl<T, R> BufferedRoutine<T, R>
where
    T: Default + Clone,
    R: Default + Clone,
{
    pub fn new(thread_count: usize) -> Self {
        {
            let buffer_bytes = size_of::<WorkBuffers<T, R>>();
            let buffer_total_bytes = buffer_bytes * thread_count;

            tracing::event!(
                tracing::Level::INFO,
                "Create buffered-routine with {thread_count} threads, each with work-buffer size {buffer_bytes} bytes on stack (x{thread_count} = {buffer_total_bytes} bytes total)"
            );
        }

        Self {
            thread_buffers: ThreadBuffers::new(thread_count),
        }
    }

    pub fn thread_buffers(&mut self) -> &mut ThreadBuffers<T, R> {
        &mut self.thread_buffers
    }

    pub fn dispatch_jobs<
        I: IntoParallelIterator,
        F: Fn(&mut WorkBuffers<T, R>, I::Item) + Sync + Send,
    >(
        &self,
        par_iter: I,
        op: F,
    ) {
        par_iter.into_par_iter().for_each(|item| {
            let work_buf = self.thread_buffers.get_current_mut();
            op(work_buf, item);
        });
    }
}
