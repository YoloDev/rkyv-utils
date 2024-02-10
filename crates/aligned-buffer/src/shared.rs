use crate::{raw::RawAlignedBuffer, UniqueAlignedBuffer};
use std::{fmt, ops, slice::SliceIndex};

/// A shared buffer with an alignment. This buffer cannot be mutated.
/// Typically, a `SharedAlignedBuffer` is created from a [`UniqueAlignedBuffer`].
/// It can be cloned and shared across threads. It is effectively the same as an
/// Arc<\[u8]>.
pub struct SharedAlignedBuffer<const ALIGNMENT: usize> {
	buf: RawAlignedBuffer<ALIGNMENT>,
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
		let buf = RawAlignedBuffer::new();
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
		self.buf.capacity()
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
}

impl<const ALIGNMENT: usize> Clone for SharedAlignedBuffer<ALIGNMENT> {
	fn clone(&self) -> Self {
		Self {
			buf: self.buf.ref_clone(),
		}
	}
}

impl<const ALIGNMENT: usize> From<UniqueAlignedBuffer<ALIGNMENT>>
	for SharedAlignedBuffer<ALIGNMENT>
{
	fn from(mut buf: UniqueAlignedBuffer<ALIGNMENT>) -> Self {
		buf.shrink_to_fit();
		debug_assert_eq!(buf.buf.capacity(), buf.len());
		Self { buf: buf.buf }
	}
}

impl<const ALIGNMENT: usize> ops::Deref for SharedAlignedBuffer<ALIGNMENT> {
	type Target = [u8];

	#[inline]
	fn deref(&self) -> &Self::Target {
		unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len()) }
	}
}

impl<I: SliceIndex<[u8]>, const ALIGNMENT: usize> ops::Index<I> for SharedAlignedBuffer<ALIGNMENT> {
	type Output = I::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		ops::Index::index(&**self, index)
	}
}

impl<const ALIGNMENT: usize> fmt::Debug for SharedAlignedBuffer<ALIGNMENT> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(&**self, f)
	}
}

impl<const ALIGNMENT: usize> AsRef<[u8]> for SharedAlignedBuffer<ALIGNMENT> {
	#[inline]
	fn as_ref(&self) -> &[u8] {
		self
	}
}

#[cfg(feature = "stable-deref-trait")]
unsafe impl<const ALIGNMENT: usize> stable_deref_trait::StableDeref
	for SharedAlignedBuffer<ALIGNMENT>
{
}

#[cfg(feature = "stable-deref-trait")]
unsafe impl<const ALIGNMENT: usize> stable_deref_trait::CloneStableDeref
	for SharedAlignedBuffer<ALIGNMENT>
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

	// Check that the `SharedAlignedBuffer` is `Send` and `Sync`.
	const _: () = {
		const fn assert_send_sync<T: Send + Sync>() {}
		assert_send_sync::<SharedAlignedBuffer<16>>();
	};
}
