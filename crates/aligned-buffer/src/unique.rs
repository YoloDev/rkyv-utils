use crate::{
	alloc::{BufferAllocator, Global},
	cap::Cap,
	raw::{RawAlignedBuffer, RawBufferError},
	SharedAlignedBuffer, DEFAULT_BUFFER_ALIGNMENT,
};
use core::fmt;
use std::{
	cmp, ops,
	ptr::{self, NonNull},
	slice::SliceIndex,
};

#[derive(Debug, thiserror::Error)]
#[error("failed to reserve capacity")]
pub struct TryReserveError {
	#[source]
	source: Box<RawBufferError>,
}

impl From<RawBufferError> for TryReserveError {
	#[inline]
	fn from(source: RawBufferError) -> Self {
		Self {
			source: Box::new(source),
		}
	}
}

/// A unique (owned) aligned buffer. This can be used to write data to the buffer,
/// before converting it to a [`SharedAlignedBuffer`] to get cheap clones and sharing
/// of the buffer data. This type is effectively a `Vec<u8>` with a custom alignment.
///
/// [`SharedAlignedBuffer`]: crate::SharedAlignedBuffer
pub struct UniqueAlignedBuffer<const ALIGNMENT: usize = DEFAULT_BUFFER_ALIGNMENT, A = Global>
where
	A: BufferAllocator<ALIGNMENT>,
{
	pub(crate) buf: RawAlignedBuffer<ALIGNMENT, A>,
	pub(crate) len: usize,
}

impl<const ALIGNMENT: usize> UniqueAlignedBuffer<ALIGNMENT> {
	/// Constructs a new, empty `UniqueAlignedBuffer`.
	///
	/// The buffer will not allocate until elements are pushed onto it.
	///
	/// # Examples
	///
	/// ```
	/// # #![allow(unused_mut)]
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::new();
	/// ```
	#[inline]
	#[must_use]
	pub const fn new() -> Self {
		Self::new_in(Global)
	}

	/// Constructs a new, empty `UniqueAlignedBuffer` with at least the specified capacity.
	///
	/// The buffer will be able to hold at least `capacity` elements without
	/// reallocating. This method is allowed to allocate for more elements than
	/// `capacity`. If `capacity` is 0, the vector will not allocate.
	///
	/// It is important to note that although the returned vector has the
	/// minimum *capacity* specified, the vector will have a zero *length*. For
	/// an explanation of the difference between length and capacity, see
	/// *[Capacity and reallocation]*.
	///
	/// If it is important to know the exact allocated capacity of a `UniqueAlignedBuffer`,
	/// always use the [`capacity`] method after construction.
	///
	/// [Capacity and reallocation]: #capacity-and-reallocation
	/// [`capacity`]: UniqueAlignedBuffer::capacity
	///
	/// # Panics
	///
	/// Panics if the new capacity is too large.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	///
	/// // The vector contains no items, even though it has capacity for more
	/// assert_eq!(buf.len(), 0);
	/// assert!(buf.capacity() >= 10);
	///
	/// // These are all done without reallocating...
	/// for i in 0..10 {
	///     buf.push(i);
	/// }
	/// assert_eq!(buf.len(), 10);
	/// assert!(buf.capacity() >= 10);
	///
	/// // ...but this may make the vector reallocate
	/// buf.push(11);
	/// assert_eq!(buf.len(), 11);
	/// assert!(buf.capacity() >= 11);
	/// ```
	#[inline]
	#[must_use]
	pub fn with_capacity(capacity: usize) -> Self {
		Self::with_capacity_in(capacity, Global)
	}

	/// Decomposes a `UniqueAlignedBuffer` into its raw components.
	///
	/// Returns the raw pointer to the underlying data, the length of
	/// the buffer, and the allocated capacity of the buffer.
	/// These are the same arguments in the same
	/// order as the arguments to [`from_raw_parts`].
	///
	/// After calling this function, the caller is responsible for the
	/// memory previously managed by the `UniqueAlignedBuffer`. The only
	/// ways to do this are to convert the raw pointer, length, and capacity
	/// back into a `UniqueAlignedBuffer` with the [`from_raw_parts`] function.
	///
	/// Note that it is valid to shrink the length of the buffer (even set it
	/// to zero) and call `from_raw_parts` with the reduced length. This is
	/// effectively the same as calling [`truncate`] or [`set_len`].
	///
	/// [`from_raw_parts`]: UniqueAlignedBuffer::from_raw_parts
	/// [`truncate`]: UniqueAlignedBuffer::truncate
	/// [`set_len`]: UniqueAlignedBuffer::set_len
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	///
	/// assert_eq!(&*buf, &[1, 2, 3]);
	/// let (ptr, len, cap) = buf.into_raw_parts();
	///
	/// let rebuilt = unsafe {
	///     UniqueAlignedBuffer::<32>::from_raw_parts(ptr, 2, cap)
	/// };
	/// assert_eq!(&*rebuilt, &[1, 2]);
	/// ```
	pub fn into_raw_parts(self) -> (NonNull<u8>, usize, Cap) {
		let (ptr, cap) = self.buf.into_raw_parts();
		(ptr, self.len, cap)
	}

	/// Creates a `UniqueAlignedBuffer` directly from a pointer, a capacity, and a length.
	///
	/// # Safety
	///
	/// This is highly unsafe, due to the number of invariants that aren't
	/// checked:
	///
	/// * `ptr` must have been allocated using the global allocator, such as via
	///   the [`alloc::alloc`] function.
	/// * `ptr` needs to be correctly offset into the allocation based on `ALIGNMENT`.
	/// * `ptr` needs to point to an allocation with the correct size.
	/// * In front of `ptr` there is a valid `RawAlignedBuffer` header.
	/// * `length` needs to be less than or equal to `capacity`.
	/// * The first `length` bytes must be properly initialized.
	/// * `capacity` needs to be the capacity that the pointer was allocated with.
	/// * The allocated size in bytes must be no larger than `isize::MAX`.
	///   See the safety documentation of [`pointer::offset`].
	///
	/// These requirements are always upheld by any `ptr` that has been allocated
	/// via `UniqueAlignedBuffer`. Other allocation sources are allowed if the invariants are
	/// upheld.
	///
	/// Violating these may cause problems like corrupting the allocator's
	/// internal data structures. For example it is normally **not** safe
	/// to build a `UniqueAlignedBuffer` from a pointer to a C `char` array with length
	/// `size_t`, doing so is only safe if the array was initially allocated by
	/// a `UniqueAlignedBuffer`.
	///
	/// The ownership of `ptr` is effectively transferred to the
	/// `UniqueAlignedBuffer` which may then deallocate, reallocate or change the
	/// contents of memory pointed to by the pointer at will. Ensure
	/// that nothing else uses the pointer after calling this
	/// function.
	///
	/// [`alloc::alloc`]: alloc::alloc::alloc
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	///
	/// assert_eq!(&*buf, &[1, 2, 3]);
	/// let (ptr, len, cap) = buf.into_raw_parts();
	///
	/// let rebuilt = unsafe {
	///     UniqueAlignedBuffer::<32>::from_raw_parts(ptr, 2, cap)
	/// };
	/// assert_eq!(&*rebuilt, &[1, 2]);
	/// ```
	#[inline]
	pub unsafe fn from_raw_parts(ptr: NonNull<u8>, len: usize, capacity: Cap) -> Self {
		let buf = RawAlignedBuffer::from_raw_parts(ptr, capacity);
		Self { buf, len }
	}
}

impl<const ALIGNMENT: usize, A> UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	/// Constructs a new, empty `UniqueAlignedBuffer`.
	///
	/// The buffer will not allocate until elements are pushed onto it.
	///
	/// # Examples
	///
	/// ```
	/// # #![allow(unused_mut)]
	/// # use aligned_buffer::{UniqueAlignedBuffer, alloc::Global};
	/// let mut buf = UniqueAlignedBuffer::<32>::new_in(Global);
	/// ```
	#[inline]
	#[must_use]
	pub const fn new_in(alloc: A) -> Self {
		let buf = RawAlignedBuffer::new_in(alloc);
		Self { buf, len: 0 }
	}

	/// Constructs a new, empty `UniqueAlignedBuffer` with at least the specified capacity.
	///
	/// The buffer will be able to hold at least `capacity` elements without
	/// reallocating. This method is allowed to allocate for more elements than
	/// `capacity`. If `capacity` is 0, the vector will not allocate.
	///
	/// It is important to note that although the returned vector has the
	/// minimum *capacity* specified, the vector will have a zero *length*. For
	/// an explanation of the difference between length and capacity, see
	/// *[Capacity and reallocation]*.
	///
	/// If it is important to know the exact allocated capacity of a `UniqueAlignedBuffer`,
	/// always use the [`capacity`] method after construction.
	///
	/// [Capacity and reallocation]: #capacity-and-reallocation
	/// [`capacity`]: UniqueAlignedBuffer::capacity
	///
	/// # Panics
	///
	/// Panics if the new capacity is too large.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::{UniqueAlignedBuffer, alloc::Global};
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity_in(10, Global);
	///
	/// // The vector contains no items, even though it has capacity for more
	/// assert_eq!(buf.len(), 0);
	/// assert!(buf.capacity() >= 10);
	///
	/// // These are all done without reallocating...
	/// for i in 0..10 {
	///     buf.push(i);
	/// }
	/// assert_eq!(buf.len(), 10);
	/// assert!(buf.capacity() >= 10);
	///
	/// // ...but this may make the vector reallocate
	/// buf.push(11);
	/// assert_eq!(buf.len(), 11);
	/// assert!(buf.capacity() >= 11);
	/// ```
	#[inline]
	#[must_use]
	pub fn with_capacity_in(capacity: usize, alloc: A) -> Self {
		let buf = RawAlignedBuffer::with_capacity_in(capacity, alloc);
		Self { buf, len: 0 }
	}

	/// Decomposes a `UniqueAlignedBuffer` into its raw components.
	///
	/// Returns the raw pointer to the underlying data, the length of
	/// the buffer, and the allocated capacity of the buffer.
	/// These are the same arguments in the same
	/// order as the arguments to [`from_raw_parts_in`].
	///
	/// After calling this function, the caller is responsible for the
	/// memory previously managed by the `UniqueAlignedBuffer`. The only
	/// ways to do this are to convert the raw pointer, length, and capacity
	/// back into a `UniqueAlignedBuffer` with the [`from_raw_parts`] function.
	///
	/// Note that it is valid to shrink the length of the buffer (even set it
	/// to zero) and call `from_raw_parts` with the reduced length. This is
	/// effectively the same as calling [`truncate`] or [`set_len`].
	///
	/// [`from_raw_parts_in`]: UniqueAlignedBuffer::from_raw_parts_in
	/// [`truncate`]: UniqueAlignedBuffer::truncate
	/// [`set_len`]: UniqueAlignedBuffer::set_len
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	///
	/// assert_eq!(&*buf, &[1, 2, 3]);
	/// let (ptr, len, cap, alloc) = buf.into_raw_parts_with_alloc();
	///
	/// let rebuilt = unsafe {
	///     UniqueAlignedBuffer::<32>::from_raw_parts_in(ptr, 2, cap, alloc)
	/// };
	/// assert_eq!(&*rebuilt, &[1, 2]);
	/// ```
	pub fn into_raw_parts_with_alloc(self) -> (NonNull<u8>, usize, Cap, A) {
		let (ptr, cap, alloc) = self.buf.into_raw_parts_with_alloc();
		(ptr, self.len, cap, alloc)
	}

	/// Creates a `UniqueAlignedBuffer` directly from a pointer, a capacity, and a length.
	///
	/// # Safety
	///
	/// This is highly unsafe, due to the number of invariants that aren't
	/// checked:
	///
	/// * `ptr` must have been allocated using the global allocator, such as via
	///   the [`alloc::alloc`] function.
	/// * `ptr` needs to be correctly offset into the allocation based on `ALIGNMENT`.
	/// * `ptr` needs to point to an allocation with the correct size.
	/// * In front of `ptr` there is a valid `RawAlignedBuffer` header.
	/// * `length` needs to be less than or equal to `capacity`.
	/// * The first `length` bytes must be properly initialized.
	/// * `capacity` needs to be the capacity that the pointer was allocated with.
	/// * The allocated size in bytes must be no larger than `isize::MAX`.
	///   See the safety documentation of [`pointer::offset`].
	///
	/// These requirements are always upheld by any `ptr` that has been allocated
	/// via `UniqueAlignedBuffer`. Other allocation sources are allowed if the invariants are
	/// upheld.
	///
	/// Violating these may cause problems like corrupting the allocator's
	/// internal data structures. For example it is normally **not** safe
	/// to build a `UniqueAlignedBuffer` from a pointer to a C `char` array with length
	/// `size_t`, doing so is only safe if the array was initially allocated by
	/// a `UniqueAlignedBuffer`.
	///
	/// The ownership of `ptr` is effectively transferred to the
	/// `UniqueAlignedBuffer` which may then deallocate, reallocate or change the
	/// contents of memory pointed to by the pointer at will. Ensure
	/// that nothing else uses the pointer after calling this
	/// function.
	///
	/// [`alloc::alloc`]: alloc::alloc::alloc
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	///
	/// assert_eq!(&*buf, &[1, 2, 3]);
	/// let (ptr, len, cap, alloc) = buf.into_raw_parts_with_alloc();
	///
	/// let rebuilt = unsafe {
	///     UniqueAlignedBuffer::<32>::from_raw_parts_in(ptr, 2, cap, alloc)
	/// };
	/// assert_eq!(&*rebuilt, &[1, 2]);
	/// ```
	#[inline]
	pub unsafe fn from_raw_parts_in(ptr: NonNull<u8>, len: usize, capacity: Cap, alloc: A) -> Self {
		let buf = RawAlignedBuffer::from_raw_parts_in(ptr, capacity, alloc);
		Self { buf, len }
	}

	/// Returns the total number of elements the buffer can hold without
	/// reallocating.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.push(42);
	/// assert!(buf.capacity() >= 10);
	/// ```
	#[inline]
	pub fn capacity(&self) -> usize {
		// when the buffer is owned (unique), cap_or_len is the capacity
		self.buf.cap_or_len()
	}

	/// Reserves capacity for at least `additional` more elements to be inserted
	/// in the given `UniqueAlignedBuffer`. The collection may reserve more space to
	/// speculatively avoid frequent reallocations. After calling `reserve`,
	/// capacity will be greater than or equal to `self.len() + additional`.
	/// Does nothing if capacity is already sufficient.
	///
	/// # Panics
	///
	/// Panics if the new capacity is too large.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.reserve(20);
	/// assert!(buf.capacity() >= 20);
	/// ```
	pub fn reserve(&mut self, additional: usize) {
		// SAFETY: We're the unieue owner of the buffer.
		unsafe {
			self.buf.reserve(self.len, additional);
		}
	}

	/// Reserves the minimum capacity for at least `additional` more elements to
	/// be inserted in the given `UniqueAlignedBuffer`. Unlike [`reserve`], this will not
	/// deliberately over-allocate to speculatively avoid frequent allocations.
	/// After calling `reserve_exact`, capacity will be greater than or equal to
	/// `self.len() + additional`. Does nothing if the capacity is already
	/// sufficient.
	///
	/// Note that the allocator may give the collection more space than it
	/// requests. Therefore, capacity can not be relied upon to be precisely
	/// minimal. Prefer [`reserve`] if future insertions are expected.
	///
	/// [`reserve`]: UniqueAlignedBuffer::reserve
	///
	/// # Panics
	///
	/// Panics if the new capacity is too large.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<32>::with_capacity(10);
	/// buf.reserve_exact(20);
	/// assert!(buf.capacity() >= 20);
	/// ```
	pub fn reserve_exact(&mut self, additional: usize) {
		// SAFETY: We're the unieue owner of the buffer.
		unsafe {
			self.buf.reserve_exact(self.len, additional);
		}
	}

	/// Tries to reserve capacity for at least `additional` more elements to be inserted
	/// in the given `UniqueAlignedBuffer`. The collection may reserve more space to speculatively avoid
	/// frequent reallocations. After calling `try_reserve`, capacity will be
	/// greater than or equal to `self.len() + additional` if it returns
	/// `Ok(())`. Does nothing if capacity is already sufficient. This method
	/// preserves the contents even if an error occurs.
	///
	/// # Errors
	///
	/// If the capacity overflows, or the allocator reports a failure, then an error
	/// is returned.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// # use aligned_buffer::TryReserveError;
	/// fn process_data(data: &[u32]) -> Result<UniqueAlignedBuffer<64>, TryReserveError> {
	///     let mut output = UniqueAlignedBuffer::<64>::new();
	///
	///     // Pre-reserve the memory, exiting if we can't
	///     output.try_reserve(data.len() * std::mem::size_of::<u32>())?;
	///
	///     // Now we know this can't OOM in the middle of our complex work
	///     output.extend(data.iter().flat_map(|&val| {
	///         u32::to_le_bytes(val * 2 + 5) // very complicated
	///     }));
	///
	///     Ok(output)
	/// }
	/// # process_data(&[1, 2, 3]).expect("why is the test harness OOMing on 12 bytes?");
	/// ```
	pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
		// SAFETY: We're the unieue owner of the buffer.
		unsafe {
			self
				.buf
				.try_reserve(self.len, additional)
				.map_err(TryReserveError::from)
		}
	}

	/// Tries to reserve the minimum capacity for at least `additional`
	/// elements to be inserted in the given `UniqueAlignedBuffer`. Unlike [`try_reserve`],
	/// this will not deliberately over-allocate to speculatively avoid frequent
	/// allocations. After calling `try_reserve_exact`, capacity will be greater
	/// than or equal to `self.len() + additional` if it returns `Ok(())`.
	/// Does nothing if the capacity is already sufficient.
	///
	/// Note that the allocator may give the collection more space than it
	/// requests. Therefore, capacity can not be relied upon to be precisely
	/// minimal. Prefer [`try_reserve`] if future insertions are expected.
	///
	/// [`try_reserve`]: Vec::try_reserve
	///
	/// # Errors
	///
	/// If the capacity overflows, or the allocator reports a failure, then an error
	/// is returned.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// # use aligned_buffer::TryReserveError;
	///
	/// fn process_data(data: &[u32]) -> Result<UniqueAlignedBuffer<64>, TryReserveError> {
	///     let mut output = UniqueAlignedBuffer::<64>::new();
	///
	///     // Pre-reserve the memory, exiting if we can't
	///     output.try_reserve_exact(data.len() * std::mem::size_of::<u32>())?;
	///
	///     // Now we know this can't OOM in the middle of our complex work
	///     output.extend(data.iter().flat_map(|&val| {
	///         u32::to_le_bytes(val * 2 + 5) // very complicated
	///     }));
	///
	///     Ok(output)
	/// }
	/// # process_data(&[1, 2, 3]).expect("why is the test harness OOMing on 12 bytes?");
	/// ```
	pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
		// SAFETY: We're the unieue owner of the buffer.
		unsafe {
			self
				.buf
				.try_reserve_exact(self.len, additional)
				.map_err(TryReserveError::from)
		}
	}

	/// Shrinks the capacity of the buffer as much as possible.
	///
	/// It will drop down as close as possible to the length but the allocator
	/// may still inform the buffer that there is space for a few more elements.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	/// assert!(buf.capacity() >= 10);
	/// buf.shrink_to_fit();
	/// assert!(buf.capacity() >= 3);
	/// ```
	pub fn shrink_to_fit(&mut self) {
		// The capacity is never less than the length, and there's nothing to do when
		// they are equal, so we can avoid the panic case in `RawVec::shrink_to_fit`
		// by only calling it with a greater capacity.
		if self.capacity() > self.len {
			// SAFETY: We're the unieue owner of the buffer.
			unsafe {
				self.buf.shrink_to_fit(self.len);
			}
		}
	}

	/// Shrinks the capacity of the buffer with a lower bound.
	///
	/// The capacity will remain at least as large as both the length
	/// and the supplied value.
	///
	/// If the current capacity is less than the lower limit, this is a no-op.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	/// assert!(buf.capacity() >= 10);
	/// buf.shrink_to(4);
	/// assert!(buf.capacity() >= 4);
	/// buf.shrink_to(0);
	/// assert!(buf.capacity() >= 3);
	/// ```
	pub fn shrink_to(&mut self, min_capacity: usize) {
		if self.capacity() > min_capacity {
			// SAFETY: We're the unieue owner of the buffer.
			unsafe {
				self.buf.shrink_to_fit(cmp::max(self.len, min_capacity));
			}
		}
	}

	/// Shortens the buffer, keeping the first `len` elements and dropping
	/// the rest.
	///
	/// If `len` is greater or equal to the vector's current length, this has
	/// no effect.
	///
	/// Note that this method has no effect on the allocated capacity
	/// of the buffer.
	///
	/// # Examples
	///
	/// Truncating a five element buffer to two elements:
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 4, 5]);
	/// buf.truncate(2);
	/// assert_eq!(&*buf, &[1, 2]);
	/// ```
	///
	/// No truncation occurs when `len` is greater than the vector's current
	/// length:
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 4, 5]);
	/// buf.truncate(8);
	/// assert_eq!(&*buf, &[1, 2, 3, 4, 5]);
	/// ```
	///
	/// Truncating when `len == 0` is equivalent to calling the [`clear`]
	/// method.
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 4, 5]);
	/// buf.truncate(0);
	/// assert_eq!(&*buf, &[]);
	/// assert!(buf.is_empty());
	/// ```
	///
	/// [`clear`]: UniqueAlignedBuffer::clear
	pub fn truncate(&mut self, len: usize) {
		// Since we're dealing with plain old data, we can just change the len
		// without having to drop anything.
		self.len = cmp::min(len, self.len);
	}

	/// Extracts a slice containing the entire buffer.
	///
	/// Equivalent to `&s[..]`.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// use std::io::{self, Write};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 5, 8]);
	/// io::sink().write(buf.as_slice()).unwrap();
	/// ```
	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self
	}

	/// Extracts a mutable slice of the entire buffer.
	///
	/// Equivalent to `&mut s[..]`.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// use std::io::{self, Read};
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([0; 3]);
	/// io::repeat(0b101).read_exact(buf.as_mut_slice()).unwrap();
	/// ```
	#[inline]
	pub fn as_mut_slice(&mut self) -> &mut [u8] {
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
	/// The caller must also ensure that the memory the pointer (non-transitively) points to
	/// is never written to using this pointer or any pointer derived from it. If you need to
	/// mutate the contents of the slice, use [`as_mut_ptr`].
	///
	/// This method guarantees that for the purpose of the aliasing model, this method
	/// does not materialize a reference to the underlying slice, and thus the returned pointer
	/// will remain valid when mixed with other calls to [`as_ptr`] and [`as_mut_ptr`].
	/// Note that calling other methods that materialize mutable references to the slice,
	/// or mutable references to specific elements you are planning on accessing through this pointer,
	/// as well as writing to those elements, may still invalidate this pointer.
	/// See the second example below for how this guarantee can be used.
	///
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 4]);
	/// let buf_ptr = buf.as_ptr();
	///
	/// unsafe {
	///     for i in 0..buf.len() {
	///         assert_eq!(*buf_ptr.add(i), 1 << i);
	///     }
	/// }
	/// ```
	///
	/// Due to the aliasing guarantee, the following code is legal:
	///
	/// ```rust
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// unsafe {
	///     let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	///     buf.extend([1, 2, 4]);
	///     let ptr1 = buf.as_ptr();
	///     let _ = ptr1.read();
	///     let ptr2 = buf.as_mut_ptr().offset(2);
	///     ptr2.write(2);
	///     // Notably, the write to `ptr2` did *not* invalidate `ptr1`
	///     // because it mutated a different element:
	///     let _ = ptr1.read();
	/// }
	/// ```
	///
	/// [`as_mut_ptr`]: UniqueAlignedBuffer::as_mut_ptr
	/// [`as_ptr`]: UniqueAlignedBuffer::as_ptr
	#[inline]
	pub fn as_ptr(&self) -> *const u8 {
		// We shadow the slice method of the same name to avoid going through
		// `deref`, which creates an intermediate reference.
		self.buf.ptr()
	}

	/// Returns an unsafe mutable pointer to the buffer's data, or a dangling
	/// raw pointer valid for zero sized reads if the buffer didn't allocate.
	///
	/// The caller must ensure that the buffer outlives the pointer this
	/// function returns, or else it will end up pointing to garbage.
	/// Modifying the vector may cause its buffer to be reallocated,
	/// which would also make any pointers to it invalid.
	///
	/// This method guarantees that for the purpose of the aliasing model, this method
	/// does not materialize a reference to the underlying slice, and thus the returned pointer
	/// will remain valid when mixed with other calls to [`as_ptr`] and [`as_mut_ptr`].
	/// Note that calling other methods that materialize references to the slice,
	/// or references to specific elements you are planning on accessing through this pointer,
	/// may still invalidate this pointer.
	/// See the second example below for how this guarantee can be used.
	///
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// // Allocate buffer big enough for 4 elements.
	/// let size = 4;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(size);
	/// let buf_ptr = buf.as_mut_ptr();
	///
	/// // Initialize elements via raw pointer writes, then set length.
	/// unsafe {
	///     for i in 0..size {
	///         *buf_ptr.add(i) = i as u8;
	///     }
	///     buf.set_len(size);
	/// }
	/// assert_eq!(&*buf, &[0, 1, 2, 3]);
	/// ```
	///
	/// Due to the aliasing guarantee, the following code is legal:
	///
	/// ```rust
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// unsafe {
	///     let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	///     buf.extend([0]);
	///     let ptr1 = buf.as_mut_ptr();
	///     ptr1.write(1);
	///     let ptr2 = buf.as_mut_ptr();
	///     ptr2.write(2);
	///     // Notably, the write to `ptr2` did *not* invalidate `ptr1`:
	///     ptr1.write(3);
	/// }
	/// ```
	///
	/// [`as_mut_ptr`]: UniqueAlignedBuffer::as_mut_ptr
	/// [`as_ptr`]: UniqueAlignedBuffer::as_ptr
	#[inline]
	pub fn as_mut_ptr(&mut self) -> *mut u8 {
		// We shadow the slice method of the same name to avoid going through
		// `deref_mut`, which creates an intermediate reference.
		self.buf.ptr()
	}

	/// Forces the length of the buffer to `new_len`.
	///
	/// This is a low-level operation that maintains none of the normal
	/// invariants of the type. Normally changing the length of a buffer
	/// is done using one of the safe operations instead, such as
	/// [`truncate`], [`resize`], [`extend`], or [`clear`].
	///
	/// [`truncate`]: UniqueAlignedBuffer::truncate
	/// [`resize`]: UniqueAlignedBuffer::resize
	/// [`extend`]: Extend::extend
	/// [`clear`]: UniqueAlignedBuffer::clear
	///
	/// # Safety
	///
	/// - `new_len` must be less than or equal to [`capacity()`].
	/// - The elements at `old_len..new_len` must be initialized.
	///
	/// [`capacity()`]: UniqueAlignedBuffer::capacity
	///
	/// # Examples
	///
	/// This method can be useful for situations in which the buffer
	/// is serving as a buffer for other code, particularly over FFI:
	///
	/// ```no_run
	/// # #![allow(dead_code)]
	/// # // This is just a minimal skeleton for the doc example;
	/// # // don't use this as a starting point for a real library.
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// # pub struct StreamWrapper { strm: *mut std::ffi::c_void }
	/// # const Z_OK: i32 = 0;
	/// # extern "C" {
	/// #     fn deflateGetDictionary(
	/// #         strm: *mut std::ffi::c_void,
	/// #         dictionary: *mut u8,
	/// #         dictLength: *mut usize,
	/// #     ) -> i32;
	/// # }
	/// # impl StreamWrapper {
	/// pub fn get_dictionary(&self) -> Option<UniqueAlignedBuffer<16>> {
	///     // Per the FFI method's docs, "32768 bytes is always enough".
	///     let mut dict = UniqueAlignedBuffer::<16>::with_capacity(32_768);
	///     let mut dict_length = 0;
	///     // SAFETY: When `deflateGetDictionary` returns `Z_OK`, it holds that:
	///     // 1. `dict_length` elements were initialized.
	///     // 2. `dict_length` <= the capacity (32_768)
	///     // which makes `set_len` safe to call.
	///     unsafe {
	///         // Make the FFI call...
	///         let r = deflateGetDictionary(self.strm, dict.as_mut_ptr(), &mut dict_length);
	///         if r == Z_OK {
	///             // ...and update the length to what was initialized.
	///             dict.set_len(dict_length);
	///             Some(dict)
	///         } else {
	///             None
	///         }
	///     }
	/// }
	/// # }
	/// ```
	///
	/// Normally, here, one would use [`clear`] instead to correctly drop
	/// the contents and thus not leak memory.
	#[inline]
	pub unsafe fn set_len(&mut self, new_len: usize) {
		debug_assert!(new_len <= self.capacity());

		self.len = new_len;
	}

	/// Appends an element to the back of a buffer.
	///
	/// # Panics
	///
	/// Panics if the new capacity is too large.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2]);
	/// buf.push(3);
	/// assert_eq!(&*buf, &[1, 2, 3]);
	/// ```
	#[inline]
	pub fn push(&mut self, value: u8) {
		// This will panic or abort if we would allocate too much.
		if self.len == self.capacity() {
			// SAFETY: We're the unieue owner of the buffer.
			unsafe {
				self.buf.reserve(self.len, 1);
			}
		}

		unsafe {
			let end = self.as_mut_ptr().add(self.len);
			ptr::write(end, value);
			self.len += 1;
		}
	}

	/// Moves all the elements of `other` into `self`, leaving `other` empty.
	///
	/// # Panics
	///
	/// Panics if the new capacity exceeds `isize::MAX` bytes.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<128>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	/// let mut vec = vec![4u8, 5, 6];
	/// buf.append(&vec);
	/// assert_eq!(&*buf, &[1, 2, 3, 4, 5, 6]);
	/// assert_eq!(vec, [4, 5, 6]);
	/// ```
	#[inline]
	pub fn append(&mut self, other: &(impl AsRef<[u8]> + ?Sized)) {
		// Safety: `other` cannot overlap with `self.as_mut_slice()`,
		// because `self` is a unique reference.
		unsafe {
			self.append_elements(other.as_ref() as *const [u8]);
		}
	}

	/// Appends elements to `self` from other buffer.
	///
	/// # Safety
	/// This function requires that `other` does not overlap with `self.as_mut_slice()`.
	#[inline]
	unsafe fn append_elements(&mut self, other: *const [u8]) {
		let count = unsafe { (*other).len() };
		self.reserve(count);
		let len = self.len();
		unsafe { ptr::copy_nonoverlapping(other as *const u8, self.as_mut_ptr().add(len), count) };
		self.len += count;
	}

	/// Clears the buffer, removing all values.
	///
	/// Note that this method has no effect on the allocated capacity
	/// of the buffer.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	///
	/// buf.clear();
	///
	/// assert!(buf.is_empty());
	/// ```
	#[inline]
	pub fn clear(&mut self) {
		self.len = 0;
	}

	/// Returns the number of elements in the buffer, also referred to
	/// as its 'length'.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3]);
	/// assert_eq!(buf.len(), 3);
	/// ```
	#[inline]
	pub fn len(&self) -> usize {
		self.len
	}

	/// Returns `true` if the buffer contains no data.
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// assert!(buf.is_empty());
	///
	/// buf.push(1);
	/// assert!(!buf.is_empty());
	/// ```
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Resizes the `UniqueAlignedBuffer` in-place so that `len` is equal to `new_len`.
	///
	/// If `new_len` is greater than `len`, the `UniqueAlignedBuffer` is extended by the
	/// difference, with each additional slot filled with `value`.
	/// If `new_len` is less than `len`, the `UniqueAlignedBuffer` is simply truncated.
	///
	/// If you only need to resize to a smaller size, use [`UniqueAlignedBuffer::truncate`].
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.resize(3, 42);
	/// assert_eq!(&*buf, &[42, 42, 42]);
	///
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1, 2, 3, 4]);
	/// buf.resize(2, 0);
	/// assert_eq!(&*buf, &[1, 2]);
	/// ```
	pub fn resize(&mut self, new_len: usize, value: u8) {
		let len = self.len();

		if new_len > len {
			self.extend_with(new_len - len, value)
		} else {
			self.truncate(new_len);
		}
	}

	/// Copies and appends all elements in a slice to the `UniqueAlignedBuffer`.
	///
	/// Iterates over the slice `other`, copies each element, and then appends
	/// it to this `UniqueAlignedBuffer`. The `other` slice is traversed in-order.
	///
	/// Note that this function is same as [`extend`] except that it is
	/// specialized to work with slices instead. If and when Rust gets
	/// specialization this function will likely be deprecated (but still
	/// available).
	///
	/// # Examples
	///
	/// ```
	/// # use aligned_buffer::UniqueAlignedBuffer;
	/// let mut buf = UniqueAlignedBuffer::<16>::with_capacity(10);
	/// buf.extend([1]);
	/// buf.extend_from_slice(&[2, 3, 4]);
	/// assert_eq!(&*buf, &[1, 2, 3, 4]);
	/// ```
	///
	/// [`extend`]: UniqueAlignedBuffer::extend
	pub fn extend_from_slice(&mut self, other: &[u8]) {
		self.append(other);
	}

	/// Extend the vector by `n` clones of value.
	fn extend_with(&mut self, n: usize, value: u8) {
		self.reserve(n);

		unsafe {
			let mut ptr = self.as_mut_ptr().add(self.len());
			// Use SetLenOnDrop to work around bug where compiler
			// might not realize the store through `ptr` through self.set_len()
			// don't alias.
			let mut local_len = SetLenOnDrop::new(&mut self.len);

			// Write all elements
			for _ in 0..n {
				ptr::write(ptr, value);
				ptr = ptr.add(1);
			}

			local_len.increment_len(n);
			// len set by scope guard
		}
	}

	/// Converts a `UniqueAlignedBuffer` into a `SharedAlignedBuffer`
	/// that can be safely cloned and shared between threads.
	pub fn into_shared(mut self) -> SharedAlignedBuffer<ALIGNMENT, A> {
		self.buf.reset_len(self.len());
		debug_assert_eq!(self.buf.cap_or_len(), self.len());
		SharedAlignedBuffer { buf: self.buf }
	}
}

impl<const ALIGNMENT: usize, A> ops::Deref for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Target = [u8];

	#[inline]
	fn deref(&self) -> &Self::Target {
		unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len()) }
	}
}

impl<const ALIGNMENT: usize, A> ops::DerefMut for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn deref_mut(&mut self) -> &mut [u8] {
		unsafe { std::slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
	}
}

impl<I: SliceIndex<[u8]>, const ALIGNMENT: usize, A> ops::Index<I>
	for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Output = I::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		ops::Index::index(&**self, index)
	}
}

impl<I: SliceIndex<[u8]>, const ALIGNMENT: usize, A> ops::IndexMut<I>
	for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn index_mut(&mut self, index: I) -> &mut Self::Output {
		ops::IndexMut::index_mut(&mut **self, index)
	}
}

impl<const ALIGNMENT: usize> FromIterator<u8> for UniqueAlignedBuffer<ALIGNMENT, Global> {
	#[inline]
	fn from_iter<T: IntoIterator<Item = u8>>(iter: T) -> Self {
		let iter = iter.into_iter();
		let (lower, _) = iter.size_hint();
		let mut buf = Self::with_capacity(lower);
		buf.extend(iter);
		buf
	}
}

impl<const ALIGNMENT: usize, A> Extend<u8> for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
		let mut iter = iter.into_iter();
		let (lower, _) = iter.size_hint();
		self.reserve(lower);
		let free = self.capacity() - self.len();

		unsafe {
			let mut ptr = self.as_mut_ptr().add(self.len());
			// Use SetLenOnDrop to work around bug where compiler
			// might not realize the store through `ptr` through self.set_len()
			// don't alias.
			let mut local_len = SetLenOnDrop::new(&mut self.len);

			// Write elements until we run out of space or the iterator ends
			// (whichever comes first). We don't use `for-each` because we need to
			// keep the iterator alive in case not all elements fit in the
			// allocated capacity. This can happen if the iterator is not an
			// exact size iterator, or simply gives out a lower bound that is
			// not exact.
			// Note: if we could specialize on the iterator type, we could use
			// ExactSizeIterator to avoid the free check.
			for _ in 0..free {
				let Some(byte) = iter.next() else {
					// We're done, so we can just return
					return;
				};

				ptr::write(ptr, byte);
				ptr = ptr.add(1);
				// Increment the length in every step in case next() panics
				local_len.increment_len(1);
			}

			// len set by scope guard
		}

		// write the remainder of the iter using push
		for byte in iter {
			self.push(byte);
		}
	}
}

impl<const ALIGNMENT: usize, A> Default for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT> + Default,
{
	fn default() -> Self {
		Self::new_in(A::default())
	}
}

impl<const ALIGNMENT: usize, A> fmt::Debug for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(&**self, f)
	}
}

impl<const ALIGNMENT: usize, A> AsRef<[u8]> for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn as_ref(&self) -> &[u8] {
		self
	}
}

impl<const ALIGNMENT: usize, A> AsMut<[u8]> for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn as_mut(&mut self) -> &mut [u8] {
		self
	}
}

impl<const ALIGNMENT: usize, A> TryFrom<SharedAlignedBuffer<ALIGNMENT, A>>
	for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Error = SharedAlignedBuffer<ALIGNMENT, A>;

	#[inline]
	fn try_from(value: SharedAlignedBuffer<ALIGNMENT, A>) -> Result<Self, Self::Error> {
		SharedAlignedBuffer::try_unique(value)
	}
}

struct SetLenOnDrop<'a> {
	len: &'a mut usize,
	local_len: usize,
}

impl<'a> SetLenOnDrop<'a> {
	#[inline]
	pub(super) fn new(len: &'a mut usize) -> Self {
		SetLenOnDrop {
			local_len: *len,
			len,
		}
	}

	#[inline]
	pub(super) fn increment_len(&mut self, increment: usize) {
		self.local_len += increment;
	}
}

impl Drop for SetLenOnDrop<'_> {
	#[inline]
	fn drop(&mut self) {
		*self.len = self.local_len;
	}
}
