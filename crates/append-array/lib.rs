use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{Ordering, AtomicUsize};

/// Append-only fixed capacity vector.
pub struct AppendArray<T> {
    // safety invariants:
    // * data and cap may not change
    // * len may never decrease
    // * nothing may write to or invalidate *data..*data.add(len), because
    //   another thread may have a reference to it

    data: *mut T,
    cap: usize,
    len: AtomicUsize,
}

unsafe impl<T> Send for AppendArray<T> {}
unsafe impl<T> Sync for AppendArray<T> {}

impl<T> Drop for AppendArray<T> {
    fn drop(&mut self) {
        unsafe {
            // SAFETY:
            // * We have exclusive access guaranteed by &mut.
            // * `self.data` and `self.capacity` came from a Vec,
            //    so can be turned back into a Vec.
            // * `self.len` elements are properly initialized
            drop(Vec::from_raw_parts(self.data, *self.len.get_mut(), self.cap))
        }
    }
}

impl<T> AppendArray<T> {
    fn from_vec(v: Vec<T>) -> Self {
        let mut v = ManuallyDrop::new(v);
        AppendArray {
            data: v.as_mut_ptr(),
            cap: v.capacity(),
            len: AtomicUsize::new(v.len()),
        }
    }

    /// Returns the maximum number of elements this buffer can hold.
    ///
    /// The capacity can't change once allocated.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Returns the current number of elements in the buffer.
    ///
    /// This increases when the AppendArrayWriter appends to the buffer,
    /// but can never decrease.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Obtain a slice of the written part of the buffer.
    pub fn as_slice(&self) -> &[T] {
        unsafe {
            // SAFETY: This came from a vector and is properly aligned.
            // The part up to len is initialized, and won't change
            std::slice::from_raw_parts(self.data, self.len())
        }
    }
}

impl<T> Deref for AppendArray<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> From<Vec<T>> for AppendArray<T> {
    fn from(mut val: Vec<T>) -> Self {
        // We're not creating a BufferChunkWriter, so this is forever immutable
        val.shrink_to_fit();
        AppendArray::from_vec(val)
    }
}

pub struct AppendArrayWriter<T> {
    inner: Arc<AppendArray<T>>,
}

impl<T> AppendArrayWriter<T> {
    fn from_vec(v: Vec<T>) -> AppendArrayWriter<T> {
        Self {
            inner: Arc::new(AppendArray::from_vec(v))
        }
    }

    pub fn with_capacity(n: usize) -> AppendArrayWriter<T> {
        Self::from_vec(Vec::with_capacity(n))
    }

    pub fn reader(&self) -> Arc<AppendArray<T>> {
        self.inner.clone()
    }

    pub fn try_push(&mut self, val: T) -> Result<usize, T> {
        let len = self.inner.len.load(Ordering::Relaxed);
        if len < self.inner.cap {
            unsafe {
                // SAFETY:
                // * checked that position is less than capacity so
                //   address is in bounds.
                // * this is above the current len so doesn't invalidate slices
                // * this has &mut exclusive access to the only `BufferChunkWriter`
                //   wrapping `inner`, so no other thread is writing.
                self.inner.data.add(len).write(val);
            }

            self.len.store(len + 1, Ordering::Release);
            Ok(len)
        } else {
            Err(val)
        }
    }

    pub fn extend_from_slice<'a>(&mut self, slice: &'a [T]) -> &'a [T] {
        let len = self.inner.len.load(Ordering::Relaxed);
        let count = self.inner.cap.saturating_sub(len).min(slice.len());
        unsafe {
            // SAFETY:
            // * checked that position is less than capacity so
            //   address is in bounds.
            // * this is above the current len so doesn't invalidate slices
            // * this has &mut exclusive access to the only `BufferChunkWriter`
            //   wrapping `inner`, so no other thread is writing.
            self.inner.data.add(len).copy_from_nonoverlapping(slice.as_ptr(), count);
        }

        self.len.store(len + count, Ordering::Release);
        &slice[count..]
    }
}

impl<T> Deref for AppendArrayWriter<T> {
    type Target = AppendArray<T>;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<T> From<Vec<T>> for AppendArrayWriter<T> {
    fn from(vec: Vec<T>) -> AppendArrayWriter<T> {
        AppendArrayWriter::from_vec(vec)
    }
}

#[test]
fn test_push() {
    let mut writer = AppendArrayWriter::with_capacity(4);
    let reader = writer.reader();
    assert_eq!(reader.capacity(), 4);
    assert_eq!(reader.len(), 0);

    assert_eq!(writer.try_push(1), Ok(0));
    assert_eq!(reader.len(), 1);
    assert_eq!(reader.as_slice(), &[1]);

    assert_eq!(writer.try_push(2), Ok(1));
    assert_eq!(writer.try_push(3), Ok(2));
    assert_eq!(writer.try_push(4), Ok(3));
    assert_eq!(writer.try_push(5), Err(5));

    assert_eq!(reader.len(), 4);
    assert_eq!(reader.as_slice(), &[1, 2, 3, 4]);
}

#[test]
fn test_extend_from_slice() {
    let mut writer = AppendArrayWriter::with_capacity(4);
    let reader = writer.reader();
    assert_eq!(reader.capacity(), 4);
    assert_eq!(reader.len(), 0);

    assert_eq!(writer.extend_from_slice(&[1, 2]), &[]);
    assert_eq!(reader.len(), 2);
    assert_eq!(reader.as_slice(), &[1, 2]);

    assert_eq!(writer.extend_from_slice(&[3, 4, 5, 6]), &[5, 6]);
    assert_eq!(reader.len(), 4);
    assert_eq!(reader.as_slice(), &[1, 2, 3, 4]);
}
