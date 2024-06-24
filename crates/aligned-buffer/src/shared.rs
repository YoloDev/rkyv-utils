use crate::{
	alloc::{BufferAllocator, Global},
	raw::RawAlignedBuffer,
	UniqueAlignedBuffer, DEFAULT_BUFFER_ALIGNMENT,
};
use std::{fmt, ops, slice::SliceIndex};

/// A shared buffer with an alignment. This buffer cannot be mutated.
/// Typically, a `SharedAlignedBuffer` is created from a [`UniqueAlignedBuffer`].
/// It can be cloned and shared across threads. It is effectively the same as an
/// Arc<\[u8]>.
pub struct SharedAlignedBuffer<const ALIGNMENT: usize = DEFAULT_BUFFER_ALIGNMENT, A = Global>
where
	A: BufferAllocator<ALIGNMENT>,
{
	pub(crate) buf: RawAlignedBuffer<ALIGNMENT, A>,
}

impl<const ALIGNMENT: usize, A> Default for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT> + Default,
{
	#[inline]
	#[must_use]
	fn default() -> Self {
		Self::new_in(A::default())
	}
}

impl<const ALIGNMENT: usize> SharedAlignedBuffer<ALIGNMENT> {
	/// Constructs a new, empty `SharedAlignedBuffer`.
	///
	/// The buffer will not allocate and cannot be mutated.
	/// It's effectively the same as an empty slice.
	///
	/// # Examples
	///
	/// ```
	/// # #![allow(unused_mut)]
	/// # use aligned_buffer::SharedAlignedBuffer;
	/// let buf = SharedAlignedBuffer::<32>::new();
	/// ```
	#[inline]
	#[must_use]
	pub const fn new() -> Self {
		Self::new_in(Global)
	}
}

impl<const ALIGNMENT: usize, A> SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	/// Constructs a new, empty `SharedAlignedBuffer`.
	///
	/// The buffer will not allocate and cannot be mutated.
	/// It's effectively the same as an empty slice.
	///
	/// # Examples
	///
	/// ```
	/// # #![allow(unused_mut)]
	/// # use aligned_buffer::{SharedAlignedBuffer, alloc::Global};
	/// let buf = SharedAlignedBuffer::<32>::new_in(Global);
	/// ```
	#[inline]
	#[must_use]
	pub const fn new_in(alloc: A) -> Self {
		let buf = RawAlignedBuffer::new_in(alloc);
		Self { buf }
	}

	/// Extracts a slice containing the entire buffer.
	///
	/// Equivalent to `&s[..]`.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, SharedAlignedBuffer};
	/// use std::io::{self, Write};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 5, 8]);
	/// let buf = SharedAlignedBuffer::from(buf);
	/// io::sink().write(buf.as_slice()).unwrap();
	/// ```
	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self
	}

	/// Returns a raw pointer to the buffer's data, or a dangling raw pointer
	/// valid for zero sized reads if the vector didn't allocate.
	///
	/// The caller must ensure that the buffer outlives the pointer this
	/// function returns, or else it will end up pointing to garbage.
	/// Modifying the buffer may cause its buffer to be reallocated,
	/// which would also make any pointers to it invalid.
	///
	/// This method guarantees that for the purpose of the aliasing model, this method
	/// does not materialize a reference to the underlying slice, and thus the returned pointer
	/// will remain valid when mixed with other calls to [`as_ptr`].
	///
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, SharedAlignedBuffer};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 4]);
	/// let buf = SharedAlignedBuffer::from(buf);
	/// let buf_ptr = buf.as_ptr();
	///
	/// unsafe {
	///     for i in 0..buf.len() {
	///         assert_eq!(*buf_ptr.add(i), 1 << i);
	///     }
	/// }
	/// ```
	///
	/// [`as_ptr`]: SharedAlignedBuffer::as_ptr
	#[inline]
	pub fn as_ptr(&self) -> *const u8 {
		// We shadow the slice method of the same name to avoid going through
		// `deref`, which creates an intermediate reference.
		self.buf.ptr()
	}

	/// Returns the number of elements in the buffer, also referred to
	/// as its 'length'.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, SharedAlignedBuffer};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	/// let buf = SharedAlignedBuffer::from(buf);
	/// assert_eq!(buf.len(), 3);
	/// ```
	#[inline]
	pub fn len(&self) -> usize {
		// when the buffer is shared, cap_or_len is the length
		self.buf.cap_or_len()
	}

	/// Returns `true` if the buffer contains no data.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, SharedAlignedBuffer};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.push(1);
	/// let buf = SharedAlignedBuffer::from(buf);
	/// assert!(!buf.is_empty());
	/// ```
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Whether or not the buffer is unique (i.e. is the only reference to the buffer).
	#[inline]
	pub fn is_unique(&self) -> bool {
		self.buf.is_unique()
	}

	/// Returns the number of references to this buffer.
	#[inline]
	pub fn ref_count(&self) -> usize {
		self.buf.ref_count()
	}

	/// Returns a [`UniqueAlignedBuffer`] if the [`SharedAlignedBuffer`] has exactly one reference.
	///
	/// Otherwise, an [`Err`] is returned with the same [`SharedAlignedBuffer`] that was
	/// passed in.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, SharedAlignedBuffer};
	///
	/// let buf = UniqueAlignedBuffer::<16>::from_iter([1, 2, 3, 4]).into_shared();
	/// assert!(SharedAlignedBuffer::try_unique(buf).is_ok());
	///
	/// let x = UniqueAlignedBuffer::<16>::from_iter([1, 2, 3, 4]).into_shared();
	/// let _y = SharedAlignedBuffer::clone(&x);
	/// assert!(SharedAlignedBuffer::try_unique(x).is_err());
	/// ```
	pub fn try_unique(mut this: Self) -> Result<UniqueAlignedBuffer<ALIGNMENT, A>, Self> {
		if this.is_unique() {
			let len = this.len();
			this.buf.reset_cap();
			Ok(UniqueAlignedBuffer { buf: this.buf, len })
		} else {
			Err(this)
		}
	}
}

impl<const ALIGNMENT: usize, A> Clone for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT> + Clone,
{
	fn clone(&self) -> Self {
		Self {
			buf: self.buf.ref_clone(),
		}
	}
}

impl<const ALIGNMENT: usize, A> From<UniqueAlignedBuffer<ALIGNMENT, A>>
	for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn from(buf: UniqueAlignedBuffer<ALIGNMENT, A>) -> Self {
		buf.into_shared()
	}
}

impl<const ALIGNMENT: usize, A> ops::Deref for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Target = [u8];

	#[inline]
	fn deref(&self) -> &Self::Target {
		unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len()) }
	}
}

impl<I: SliceIndex<[u8]>, const ALIGNMENT: usize, A> ops::Index<I>
	for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Output = I::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		ops::Index::index(&**self, index)
	}
}

impl<const ALIGNMENT: usize, A> fmt::Debug for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(&**self, f)
	}
}

impl<const ALIGNMENT: usize, A> AsRef<[u8]> for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn as_ref(&self) -> &[u8] {
		self
	}
}

#[cfg(feature = "stable-deref-trait")]
unsafe impl<const ALIGNMENT: usize, A> stable_deref_trait::StableDeref
	for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
}

#[cfg(feature = "stable-deref-trait")]
unsafe impl<const ALIGNMENT: usize, A> stable_deref_trait::CloneStableDeref
	for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT> + Clone,
{
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn clones_returns_same_pointer() {
		let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
		buf.extend([1, 2, 3]);
		let buf = SharedAlignedBuffer::from(buf);
		let buf2 = buf.clone();
		assert_eq!(buf.as_ptr(), buf2.as_ptr());
	}

	#[test]
	fn try_unique_returns_err_when_not_unique() {
		let x = UniqueAlignedBuffer::<16>::from_iter([1, 2, 3, 4]).into_shared();
		let _y = SharedAlignedBuffer::clone(&x);
		assert!(SharedAlignedBuffer::try_unique(x).is_err());
	}

	#[test]
	fn sharing_does_not_shrink_the_buffer() {
		let buf = UniqueAlignedBuffer::<64>::with_capacity(10);
		let cap = buf.capacity();
		let buf = buf.into_shared();
		assert_eq!(&*buf, &[]);

		let buf = UniqueAlignedBuffer::try_from(buf).unwrap();
		assert_eq!(&*buf, &[]);
		assert_eq!(buf.capacity(), cap);
	}

	// Check that the `SharedAlignedBuffer` is `Send` and `Sync`.
	const _: () = {
		const fn assert_send_sync<T: Send + Sync>() {}
		assert_send_sync::<SharedAlignedBuffer<16>>();
	};
}
