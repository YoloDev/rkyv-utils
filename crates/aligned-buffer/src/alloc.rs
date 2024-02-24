use crate::cap::Cap;
use std::{
	alloc::{self, Layout},
	ptr::{self, NonNull},
};

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
#[error("memory allocation failed")]
pub struct AllocError;

/// Stable version of the Allocator trait.
///
/// This trait is a copy of the unstable `std::alloc::Allocator` trait. It is intended to be used as
/// a stable version of the unstable trait. Once the unstable trait is stabilized, this trait will be
/// removed and replaced with the standard trait.
///
/// An implementation of `Allocator` can allocate, grow, shrink, and deallocate arbitrary blocks of
/// data described via [`Layout`][].
///
/// `Allocator` is designed to be implemented on ZSTs, references, or smart pointers because having
/// an allocator like `MyAlloc([u8; N])` cannot be moved, without updating the pointers to the
/// allocated memory.
///
/// Unlike [`GlobalAlloc`][], zero-sized allocations are allowed in `Allocator`. If an underlying
/// allocator does not support this (like jemalloc) or return a null pointer (such as
/// `libc::malloc`), this must be caught by the implementation.
///
/// ### Currently allocated memory
///
/// Some of the methods require that a memory block be *currently allocated* via an allocator. This
/// means that:
///
/// * the starting address for that memory block was previously returned by [`allocate`], [`grow`], or
///   [`shrink`], and
///
/// * the memory block has not been subsequently deallocated, where blocks are either deallocated
///   directly by being passed to [`deallocate`] or were changed by being passed to [`grow`] or
///   [`shrink`] that returns `Ok`. If `grow` or `shrink` have returned `Err`, the passed pointer
///   remains valid.
///
/// [`allocate`]: Allocator::allocate
/// [`grow`]: Allocator::grow
/// [`shrink`]: Allocator::shrink
/// [`deallocate`]: Allocator::deallocate
///
/// ### Memory fitting
///
/// Some of the methods require that a layout *fit* a memory block. What it means for a layout to
/// "fit" a memory block means (or equivalently, for a memory block to "fit" a layout) is that the
/// following conditions must hold:
///
/// * The block must be allocated with the same alignment as [`layout.align()`], and
///
/// * The provided [`layout.size()`] must fall in the range `min ..= max`, where:
///   - `min` is the size of the layout most recently used to allocate the block, and
///   - `max` is the latest actual size returned from [`allocate`], [`grow`], or [`shrink`].
///
/// [`layout.align()`]: Layout::align
/// [`layout.size()`]: Layout::size
///
/// # Safety
///
/// * Memory blocks returned from an allocator that are [*currently allocated*] must point to
///   valid memory and retain their validity while they are [*currently allocated*] and at
///   least one of the instance and all of its clones has not been dropped.
///
/// * copying, cloning, or moving the allocator must not invalidate memory blocks returned from this
///   allocator. A copied or cloned allocator must behave like the same allocator, and
///
/// * any pointer to a memory block which is [*currently allocated*] may be passed to any other
///   method of the allocator.
///
/// [*currently allocated*]: #currently-allocated-memory
pub unsafe trait Allocator {
	/// Attempts to allocate a block of memory.
	///
	/// On success, returns a [`NonNull<[u8]>`][NonNull] meeting the size and alignment guarantees of `layout`.
	///
	/// The returned block may have a larger size than specified by `layout.size()`, and may or may
	/// not have its contents initialized.
	///
	/// # Errors
	///
	/// Returning `Err` indicates that either memory is exhausted or `layout` does not meet
	/// allocator's size or alignment constraints.
	///
	/// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
	/// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
	/// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
	///
	/// Clients wishing to abort computation in response to an allocation error are encouraged to
	/// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
	///
	/// [`handle_alloc_error`]: std::alloc::handle_alloc_error
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError>;

	/// Behaves like `allocate`, but also ensures that the returned memory is zero-initialized.
	///
	/// # Errors
	///
	/// Returning `Err` indicates that either memory is exhausted or `layout` does not meet
	/// allocator's size or alignment constraints.
	///
	/// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
	/// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
	/// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
	///
	/// Clients wishing to abort computation in response to an allocation error are encouraged to
	/// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
	///
	/// [`handle_alloc_error`]: std::alloc::handle_alloc_error
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		let ptr = self.allocate(layout)?;
		// SAFETY: `alloc` returns a valid memory block
		unsafe { (ptr.as_ptr() as *mut u8).write_bytes(0, ptr.len()) }
		Ok(ptr)
	}

	/// Deallocates the memory referenced by `ptr`.
	///
	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator, and
	/// * `layout` must [*fit*] that block of memory.
	///
	/// [*currently allocated*]: #currently-allocated-memory
	/// [*fit*]: #memory-fitting
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout);

	/// Attempts to extend the memory block.
	///
	/// Returns a new [`NonNull<[u8]>`][NonNull] containing a pointer and the actual size of the allocated
	/// memory. The pointer is suitable for holding data described by `new_layout`. To accomplish
	/// this, the allocator may extend the allocation referenced by `ptr` to fit the new layout.
	///
	/// If this returns `Ok`, then ownership of the memory block referenced by `ptr` has been
	/// transferred to this allocator. Any access to the old `ptr` is Undefined Behavior, even if the
	/// allocation was grown in-place. The newly returned pointer is the only valid pointer
	/// for accessing this memory now.
	///
	/// If this method returns `Err`, then ownership of the memory block has not been transferred to
	/// this allocator, and the contents of the memory block are unaltered.
	///
	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
	/// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
	/// * `new_layout.size()` must be greater than or equal to `old_layout.size()`.
	///
	/// Note that `new_layout.align()` need not be the same as `old_layout.align()`.
	///
	/// [*currently allocated*]: #currently-allocated-memory
	/// [*fit*]: #memory-fitting
	///
	/// # Errors
	///
	/// Returns `Err` if the new layout does not meet the allocator's size and alignment
	/// constraints of the allocator, or if growing otherwise fails.
	///
	/// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
	/// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
	/// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
	///
	/// Clients wishing to abort computation in response to an allocation error are encouraged to
	/// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
	///
	/// [`handle_alloc_error`]: std::alloc::handle_alloc_error
	unsafe fn grow(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			new_layout.size() >= old_layout.size(),
			"`new_layout.size()` must be greater than or equal to `old_layout.size()`"
		);

		let new_ptr = self.allocate(new_layout)?;

		// SAFETY: because `new_layout.size()` must be greater than or equal to
		// `old_layout.size()`, both the old and new memory allocation are valid for reads and
		// writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
		// deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
		// safe. The safety contract for `dealloc` must be upheld by the caller.
		unsafe {
			ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, old_layout.size());
			self.deallocate(ptr, old_layout);
		}

		Ok(new_ptr)
	}

	/// Behaves like `grow`, but also ensures that the new contents are set to zero before being
	/// returned.
	///
	/// The memory block will contain the following contents after a successful call to
	/// `grow_zeroed`:
	///   * Bytes `0..old_layout.size()` are preserved from the original allocation.
	///   * Bytes `old_layout.size()..old_size` will either be preserved or zeroed, depending on
	///     the allocator implementation. `old_size` refers to the size of the memory block prior
	///     to the `grow_zeroed` call, which may be larger than the size that was originally
	///     requested when it was allocated.
	///   * Bytes `old_size..new_size` are zeroed. `new_size` refers to the size of the memory
	///     block returned by the `grow_zeroed` call.
	///
	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
	/// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
	/// * `new_layout.size()` must be greater than or equal to `old_layout.size()`.
	///
	/// Note that `new_layout.align()` need not be the same as `old_layout.align()`.
	///
	/// [*currently allocated*]: #currently-allocated-memory
	/// [*fit*]: #memory-fitting
	///
	/// # Errors
	///
	/// Returns `Err` if the new layout does not meet the allocator's size and alignment
	/// constraints of the allocator, or if growing otherwise fails.
	///
	/// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
	/// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
	/// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
	///
	/// Clients wishing to abort computation in response to an allocation error are encouraged to
	/// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
	///
	/// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
	unsafe fn grow_zeroed(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			new_layout.size() >= old_layout.size(),
			"`new_layout.size()` must be greater than or equal to `old_layout.size()`"
		);

		let new_ptr = self.allocate_zeroed(new_layout)?;

		// SAFETY: because `new_layout.size()` must be greater than or equal to
		// `old_layout.size()`, both the old and new memory allocation are valid for reads and
		// writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
		// deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
		// safe. The safety contract for `dealloc` must be upheld by the caller.
		unsafe {
			ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, old_layout.size());
			self.deallocate(ptr, old_layout);
		}

		Ok(new_ptr)
	}

	/// Attempts to shrink the memory block.
	///
	/// Returns a new [`NonNull<[u8]>`][NonNull] containing a pointer and the actual size of the allocated
	/// memory. The pointer is suitable for holding data described by `new_layout`. To accomplish
	/// this, the allocator may shrink the allocation referenced by `ptr` to fit the new layout.
	///
	/// If this returns `Ok`, then ownership of the memory block referenced by `ptr` has been
	/// transferred to this allocator. Any access to the old `ptr` is Undefined Behavior, even if the
	/// allocation was shrunk in-place. The newly returned pointer is the only valid pointer
	/// for accessing this memory now.
	///
	/// If this method returns `Err`, then ownership of the memory block has not been transferred to
	/// this allocator, and the contents of the memory block are unaltered.
	///
	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
	/// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
	/// * `new_layout.size()` must be smaller than or equal to `old_layout.size()`.
	///
	/// Note that `new_layout.align()` need not be the same as `old_layout.align()`.
	///
	/// [*currently allocated*]: #currently-allocated-memory
	/// [*fit*]: #memory-fitting
	///
	/// # Errors
	///
	/// Returns `Err` if the new layout does not meet the allocator's size and alignment
	/// constraints of the allocator, or if shrinking otherwise fails.
	///
	/// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
	/// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
	/// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
	///
	/// Clients wishing to abort computation in response to an allocation error are encouraged to
	/// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
	///
	/// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
	unsafe fn shrink(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			new_layout.size() <= old_layout.size(),
			"`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
		);

		let new_ptr = self.allocate(new_layout)?;

		// SAFETY: because `new_layout.size()` must be lower than or equal to
		// `old_layout.size()`, both the old and new memory allocation are valid for reads and
		// writes for `new_layout.size()` bytes. Also, because the old allocation wasn't yet
		// deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
		// safe. The safety contract for `dealloc` must be upheld by the caller.
		unsafe {
			ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, new_layout.size());
			self.deallocate(ptr, old_layout);
		}

		Ok(new_ptr)
	}

	/// Creates a "by reference" adapter for this instance of `Allocator`.
	///
	/// The returned adapter also implements `Allocator` and will simply borrow this.
	#[inline(always)]
	fn by_ref(&self) -> &Self
	where
		Self: Sized,
	{
		self
	}
}

unsafe impl<A> Allocator for &A
where
	A: Allocator + ?Sized,
{
	#[inline]
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		(**self).allocate(layout)
	}

	#[inline]
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		(**self).allocate_zeroed(layout)
	}

	#[inline]
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		// SAFETY: the safety contract must be upheld by the caller
		unsafe { (**self).deallocate(ptr, layout) }
	}

	#[inline]
	unsafe fn grow(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		// SAFETY: the safety contract must be upheld by the caller
		unsafe { (**self).grow(ptr, old_layout, new_layout) }
	}

	#[inline]
	unsafe fn grow_zeroed(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		// SAFETY: the safety contract must be upheld by the caller
		unsafe { (**self).grow_zeroed(ptr, old_layout, new_layout) }
	}

	#[inline]
	unsafe fn shrink(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		// SAFETY: the safety contract must be upheld by the caller
		unsafe { (**self).shrink(ptr, old_layout, new_layout) }
	}
}

/// A trait for types that can allocate aligned buffers.
pub trait BufferAllocator<const ALIGNMENT: usize>: Allocator {
	/// Deallocates the buffer.
	///
	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator, and
	/// * `layout` must [*fit*] that block of memory.
	///
	/// [*currently allocated*]: Allocator#currently-allocated-memory
	/// [*fit*]: Allocator#memory-fitting
	unsafe fn deallocate_buffer(&self, raw: RawBuffer<ALIGNMENT>) {
		let (ptr, layout) = raw.alloc_info();
		self.deallocate(ptr, layout);
	}
}

/// A raw buffer.
pub struct RawBuffer<const ALIGNMENT: usize> {
	pub(crate) buf: NonNull<u8>,
	pub(crate) cap: Cap,
}

impl<const ALIGNMENT: usize> RawBuffer<ALIGNMENT> {
	/// Creates a new raw buffer.
	///
	/// # Safety
	/// The caller must ensure that the buffer is properly aligned and has the correct capacity.
	pub(crate) unsafe fn new(buf: NonNull<u8>, capacity: Cap) -> Self {
		Self { buf, cap: capacity }
	}

	/// Returns a pointer to the start of the buffer.
	#[inline]
	pub fn buf_ptr(&self) -> *mut u8 {
		self.buf.as_ptr()
	}

	/// Returns the capacity of the buffer.
	#[inline]
	pub fn capacity(&self) -> Cap {
		self.cap
	}

	/// Returns the allocation pointer and layout.
	#[inline]
	pub fn alloc_info(&self) -> (NonNull<u8>, Layout) {
		let layout = crate::raw::RawAlignedBuffer::<ALIGNMENT>::layout(self.cap.0.value())
			.expect("Invalid layout");

		let header = unsafe {
			self
				.buf
				.as_ptr()
				.sub(crate::raw::RawAlignedBuffer::<ALIGNMENT>::BUFFER_OFFSET)
		};

		debug_assert_eq!(self.cap.0.value(), unsafe {
			(*(header as *const crate::raw::Header)).alloc_buffer_size
		});

		(unsafe { NonNull::new_unchecked(header) }, layout)
	}
}

/// Stable version of the Global allocator.
///
/// This is a copy of the unstable `std::alloc::Global` allocator. It is intended to be used as a
/// stable version of the unstable allocator. Once the unstable allocator is stabilized, this
/// allocator will be removed and replaced with the standard allocator.
///
/// The global memory allocator.
///
/// This type implements the [`Allocator`] trait by forwarding calls
/// to the allocator registered with the `#[global_allocator]` attribute
/// if there is one, or the `std` crate’s default.
#[derive(Debug, Clone, Copy, Default)]
pub struct Global;

impl Global {
	#[inline]
	fn alloc_impl(&self, layout: Layout, zeroed: bool) -> Result<NonNull<[u8]>, AllocError> {
		match layout.size() {
			0 => Ok(NonNull::slice_from_raw_parts(NonNull::dangling(), 0)),
			// SAFETY: `layout` is non-zero in size,
			size => unsafe {
				let raw_ptr = if zeroed {
					alloc::alloc_zeroed(layout)
				} else {
					alloc::alloc(layout)
				};
				let ptr = NonNull::new(raw_ptr).ok_or(AllocError)?;
				Ok(NonNull::slice_from_raw_parts(ptr, size))
			},
		}
	}

	// SAFETY: Same as `Allocator::grow`
	#[inline]
	unsafe fn grow_impl(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
		zeroed: bool,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			new_layout.size() >= old_layout.size(),
			"`new_layout.size()` must be greater than or equal to `old_layout.size()`"
		);

		match old_layout.size() {
			0 => self.alloc_impl(new_layout, zeroed),

			// SAFETY: `new_size` is non-zero as `old_size` is greater than or equal to `new_size`
			// as required by safety conditions. Other conditions must be upheld by the caller
			old_size if old_layout.align() == new_layout.align() => unsafe {
				let new_size = new_layout.size();

				let raw_ptr = alloc::realloc(ptr.as_ptr(), old_layout, new_size);
				let ptr = NonNull::new(raw_ptr).ok_or(AllocError)?;
				if zeroed {
					raw_ptr.add(old_size).write_bytes(0, new_size - old_size);
				}
				Ok(NonNull::slice_from_raw_parts(ptr, new_size))
			},

			// SAFETY: because `new_layout.size()` must be greater than or equal to `old_size`,
			// both the old and new memory allocation are valid for reads and writes for `old_size`
			// bytes. Also, because the old allocation wasn't yet deallocated, it cannot overlap
			// `new_ptr`. Thus, the call to `copy_nonoverlapping` is safe. The safety contract
			// for `dealloc` must be upheld by the caller.
			old_size => unsafe {
				let new_ptr = self.alloc_impl(new_layout, zeroed)?;
				ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, old_size);
				self.deallocate(ptr, old_layout);
				Ok(new_ptr)
			},
		}
	}
}

unsafe impl Allocator for Global {
	#[inline]
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc_impl(layout, false)
	}

	#[inline]
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc_impl(layout, true)
	}

	#[inline]
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		if layout.size() != 0 {
			// SAFETY: `layout` is non-zero in size,
			// other conditions must be upheld by the caller
			unsafe { alloc::dealloc(ptr.as_ptr(), layout) }
		}
	}

	#[inline]
	unsafe fn grow(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		// SAFETY: all conditions must be upheld by the caller
		unsafe { self.grow_impl(ptr, old_layout, new_layout, false) }
	}

	#[inline]
	unsafe fn grow_zeroed(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		// SAFETY: all conditions must be upheld by the caller
		unsafe { self.grow_impl(ptr, old_layout, new_layout, true) }
	}

	#[inline]
	unsafe fn shrink(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			new_layout.size() <= old_layout.size(),
			"`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
		);

		match new_layout.size() {
			// SAFETY: conditions must be upheld by the caller
			0 => unsafe {
				self.deallocate(ptr, old_layout);
				Ok(NonNull::slice_from_raw_parts(NonNull::dangling(), 0))
			},

			// SAFETY: `new_size` is non-zero. Other conditions must be upheld by the caller
			new_size if old_layout.align() == new_layout.align() => unsafe {
				// `realloc` probably checks for `new_size <= old_layout.size()` or something similar.

				let raw_ptr = alloc::realloc(ptr.as_ptr(), old_layout, new_size);
				let ptr = NonNull::new(raw_ptr).ok_or(AllocError)?;
				Ok(NonNull::slice_from_raw_parts(ptr, new_size))
			},

			// SAFETY: because `new_size` must be smaller than or equal to `old_layout.size()`,
			// both the old and new memory allocation are valid for reads and writes for `new_size`
			// bytes. Also, because the old allocation wasn't yet deallocated, it cannot overlap
			// `new_ptr`. Thus, the call to `copy_nonoverlapping` is safe. The safety contract
			// for `dealloc` must be upheld by the caller.
			new_size => unsafe {
				let new_ptr = self.allocate(new_layout)?;
				ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, new_size);
				self.deallocate(ptr, old_layout);
				Ok(new_ptr)
			},
		}
	}
}

impl<const ALIGNMENT: usize> BufferAllocator<ALIGNMENT> for Global {}
