mod layout;

use self::layout::LayoutHelper;
use std::{
	alloc::{self, handle_alloc_error, Layout},
	cmp, mem,
	process::abort,
	ptr::NonNull,
	sync::atomic::{self, AtomicUsize},
};

#[derive(Debug, thiserror::Error)]
pub enum RawBufferError {
	#[error("capacity overflow")]
	CapacityOverflow,

	#[non_exhaustive]
	#[error("allocation error")]
	AllocError { layout: Layout },
}

/// A soft limit on the amount of references that may be made to an `Arc`.
///
/// Going above this limit will abort your program (although not
/// necessarily) at _exactly_ `MAX_REFCOUNT + 1` references.
const MAX_REFCOUNT: usize = (isize::MAX) as usize;

#[repr(C)]
struct Header {
	ref_count: atomic::AtomicUsize,
}

impl Header {
	const LAYOUT: LayoutHelper = LayoutHelper::new::<Header>();
}

#[repr(C)]
pub(crate) struct RawAlignedBuffer<const ALIGNMENT: usize> {
	buf: NonNull<u8>,
	cap: usize,
}

unsafe impl<const ALIGNMENT: usize> Send for RawAlignedBuffer<ALIGNMENT> {}
unsafe impl<const ALIGNMENT: usize> Sync for RawAlignedBuffer<ALIGNMENT> {}

impl<const ALIGNMENT: usize> RawAlignedBuffer<ALIGNMENT> {
	// Tiny Vecs are dumb. Skip to 8, because any heap allocators is likely
	// to round up a request of less than 8 bytes to at least 8 bytes.
	const MIN_NON_ZERO_CAP: usize = 8;

	const BUFFER_OFFSET: usize = {
		let buffer_layout = LayoutHelper::from_size_alignment(0, ALIGNMENT);

		let (_, offset) = Header::LAYOUT.extend(buffer_layout);
		offset
	};

	#[inline]
	fn layout(size: usize) -> Result<Layout, RawBufferError> {
		let header_layout = Header::LAYOUT.into_layout();

		// SAFETY: We know that the alignment is valid, because else we would get compiler errors
		// instansiating BUFFER_OFFSET.
		let buffer_layout = unsafe { Layout::from_size_align_unchecked(size, ALIGNMENT) };

		let Ok((layout, offset)) = header_layout.extend(buffer_layout) else {
			return Err(RawBufferError::CapacityOverflow);
		};

		debug_assert_eq!(offset, Self::BUFFER_OFFSET);
		Ok(layout)
	}

	pub const fn new() -> Self {
		Self {
			buf: NonNull::dangling(),
			cap: 0,
		}
	}

	pub fn with_capacity(size: usize) -> Self {
		let mut buf = Self::new();

		if size == 0 {
			return buf;
		}

		// SAFETY:
		// - The size is non-zero
		// - buf is new
		handle_reserve(unsafe { buf.init(size) });

		buf
	}

	/// Gets a raw pointer to the start of the allocation. Note that this is
	/// `NonNull::dangling()` if `capacity == 0`, in which case, you must
	/// be careful.
	#[inline]
	pub fn ptr(&self) -> *mut u8 {
		self.buf.as_ptr()
	}

	/// Gets the capacity of the allocation.
	#[inline(always)]
	pub fn capacity(&self) -> usize {
		self.cap
	}

	/// Ensures that the buffer contains at least enough space to hold `len +
	/// additional` elements. If it doesn't already have enough capacity, will
	/// reallocate enough space plus comfortable slack space to get amortized
	/// *O*(1) behavior. Will limit this behavior if it would needlessly cause
	/// itself to panic.
	///
	/// This is ideal for implementing a bulk-push operation like `extend`.
	///
	/// # Panics
	///
	/// Panics if the new capacity exceeds `isize::MAX` bytes.
	///
	/// # Aborts
	///
	/// Aborts on OOM.
	///
	/// # Safety
	///
	/// Requires that the buffer only has a single reference to it.
	#[inline]
	pub unsafe fn reserve(&mut self, len: usize, additional: usize) {
		// Callers expect this function to be very cheap when there is already sufficient capacity.
		// Therefore, we move all the resizing and error-handling logic from grow_amortized and
		// handle_reserve behind a call, while making sure that this function is likely to be
		// inlined as just a comparison and a call if the comparison fails.
		#[cold]
		fn do_reserve_and_handle<const ALIGNMENT: usize>(
			slf: &mut RawAlignedBuffer<ALIGNMENT>,
			len: usize,
			additional: usize,
		) {
			handle_reserve(slf.grow_amortized(len, additional));
		}

		if self.needs_to_grow(len, additional) {
			do_reserve_and_handle(self, len, additional);
		}
	}

	/// The same as `reserve`, but returns on errors instead of panicking or aborting.
	///
	/// # Safety
	///
	/// Requires that the buffer only has a single reference to it.
	pub unsafe fn try_reserve(
		&mut self,
		len: usize,
		additional: usize,
	) -> Result<(), RawBufferError> {
		if self.needs_to_grow(len, additional) {
			self.grow_amortized(len, additional)?;
		}

		Ok(())
	}

	/// Ensures that the buffer contains at least enough space to hold `len +
	/// additional` elements. If it doesn't already, will reallocate the
	/// minimum possible amount of memory necessary. Generally this will be
	/// exactly the amount of memory necessary, but in principle the allocator
	/// is free to give back more than we asked for.
	///
	/// If `len` exceeds `self.capacity()`, this may fail to actually allocate
	/// the requested space. This is not really unsafe, but the unsafe code
	/// *you* write that relies on the behavior of this function may break.
	///
	/// # Panics
	///
	/// Panics if the new capacity exceeds `isize::MAX` bytes.
	///
	/// # Aborts
	///
	/// Aborts on OOM.
	///
	/// # Safety
	///
	/// Requires that the buffer only has a single reference to it.
	pub unsafe fn reserve_exact(&mut self, len: usize, additional: usize) {
		handle_reserve(self.try_reserve_exact(len, additional));
	}

	/// The same as `reserve_exact`, but returns on errors instead of panicking or aborting.
	///
	/// # Safety
	///
	/// Requires that the buffer only has a single reference to it.
	pub unsafe fn try_reserve_exact(
		&mut self,
		len: usize,
		additional: usize,
	) -> Result<(), RawBufferError> {
		if self.needs_to_grow(len, additional) {
			self.grow_exact(len, additional)?;
		}

		Ok(())
	}

	/// Shrinks the buffer down to the specified capacity. If the given amount
	/// is 0, actually completely deallocates.
	///
	/// # Panics
	///
	/// Panics if the given amount is *larger* than the current capacity.
	///
	/// # Aborts
	///
	/// Aborts on OOM.
	///
	/// # Safety
	///
	/// Requires that the buffer only has a single reference to it.
	pub unsafe fn shrink_to_fit(&mut self, cap: usize) {
		handle_reserve(self.shrink(cap));
	}

	/// Returns if the buffer needs to grow to fulfill the needed extra capacity.
	/// Mainly used to make inlining reserve-calls possible without inlining `grow`.
	fn needs_to_grow(&self, len: usize, additional: usize) -> bool {
		additional > self.capacity().wrapping_sub(len)
	}

	// This method is usually instantiated many times. So we want it to be as
	// small as possible, to improve compile times. But we also want as much of
	// its contents to be statically computable as possible, to make the
	// generated code run faster. Therefore, this method is carefully written
	// so that all of the code that depends on `T` is within it, while as much
	// of the code that doesn't depend on `T` as possible is in functions that
	// are non-generic over `T`.
	fn grow_amortized(&mut self, len: usize, additional: usize) -> Result<(), RawBufferError> {
		// This is ensured by the calling contexts.
		debug_assert!(additional > 0);

		// Nothing we can really do about these checks, sadly.
		let required_cap = len
			.checked_add(additional)
			.ok_or(RawBufferError::CapacityOverflow)?;

		// This guarantees exponential growth. The doubling cannot overflow
		// because `cap <= isize::MAX` and the type of `cap` is `usize`.
		let cap = cmp::max(self.cap * 2, required_cap);
		let cap = cmp::max(Self::MIN_NON_ZERO_CAP, cap);

		self.grow_to(cap)
	}

	// The constraints on this method are much the same as those on
	// `grow_amortized`, but this method is usually instantiated less often so
	// it's less critical.
	fn grow_exact(&mut self, len: usize, additional: usize) -> Result<(), RawBufferError> {
		let cap = len
			.checked_add(additional)
			.ok_or(RawBufferError::CapacityOverflow)?;

		self.grow_to(cap)
	}

	#[inline(always)]
	fn grow_to(&mut self, cap: usize) -> Result<(), RawBufferError> {
		debug_assert!(cap > self.cap, "Tried to shrink or keep the same capacity");
		if self.cap == 0 {
			// If the capacity is zero, we can just call `init` directly.
			return unsafe { self.init(cap) };
		}

		let new_layout = Self::layout(cap)?;
		alloc_guard(new_layout.size())?;

		let ptr = {
			let Ok(old_layout) = Self::layout(self.cap) else {
				unreachable!();
			};

			debug_assert_eq!(old_layout.align(), new_layout.align());

			// SAFETY:
			// - `self.ptr` is already offset by `BUFFER_OFFSET`
			// - `old_layout` is the layout that produced `self.ptr`
			// - `new_layout.size()` is already validated
			unsafe {
				let ptr = self.buf.as_ptr().sub(Self::BUFFER_OFFSET);
				alloc::realloc(ptr, old_layout, new_layout.size())
			}
		};

		if ptr.is_null() {
			return Err(RawBufferError::AllocError { layout: new_layout });
		}

		// SAFETY:
		// - The pointer is non-null
		// - The OFFSET is included in the layout that produced the pointer
		self.buf = unsafe { NonNull::new_unchecked(ptr.add(Self::BUFFER_OFFSET)) };

		// Allocators currently return a `NonNull<[u8]>` whose length
		// matches the size requested. If that ever changes, the capacity
		// here should change to `ptr.len() / mem::size_of::<T>()`.
		self.cap = cap;
		Ok(())
	}

	/// # Safety
	/// This method requires that:
	/// - `size` is non-zero
	/// - `self.ptr` is dangling
	/// - `self.cap` is zero
	unsafe fn init(&mut self, size: usize) -> Result<(), RawBufferError> {
		let layout = match Self::layout(size) {
			Ok(layout) => layout,
			Err(_) => return Err(RawBufferError::CapacityOverflow),
		};

		alloc_guard(layout.size())?;

		// SAFETY:
		// - The layout is not zero-sized, even if `size` is zero, because we extend it from the header layout.
		let ptr = unsafe { alloc::alloc(layout) };

		if ptr.is_null() {
			return Err(RawBufferError::AllocError { layout });
		}

		let header = ptr as *mut Header;
		// SAFETY: The pointer is non-null and from a new allocation, so we're not supposed to drop anything.
		unsafe {
			header.write(Header {
				ref_count: AtomicUsize::new(1),
			})
		};

		// SAFETY:
		// - The pointer is non-null
		// - The OFFSET is included in the layout that produced the pointer
		let buf = unsafe { NonNull::new_unchecked(ptr.add(Self::BUFFER_OFFSET)) };

		// Allocators currently return a `NonNull<[u8]>` whose length
		// matches the size requested. If that ever changes, the capacity
		// here should change to `ptr.len() / mem::size_of::<T>()`.
		let cap = size;
		self.buf = buf;
		self.cap = cap;

		Ok(())
	}

	fn shrink(&mut self, cap: usize) -> Result<(), RawBufferError> {
		#[cold]
		fn dealloc_buf<const ALIGNMENT: usize>(
			buf: &mut RawAlignedBuffer<ALIGNMENT>,
		) -> Result<(), RawBufferError> {
			let layout = RawAlignedBuffer::<ALIGNMENT>::layout(buf.cap)?;
			// SAFETY: The pointer is non-null and the layout is correct
			unsafe {
				alloc::dealloc(
					buf
						.buf
						.as_ptr()
						.sub(RawAlignedBuffer::<ALIGNMENT>::BUFFER_OFFSET),
					layout,
				)
			};
			buf.buf = NonNull::dangling();
			buf.cap = 0;
			Ok(())
		}

		assert!(
			cap <= self.capacity(),
			"Tried to shrink to a larger capacity"
		);

		if cap == self.capacity() {
			return Ok(());
		}

		if cap == 0 {
			return dealloc_buf(self);
		}

		let new_layout = Self::layout(cap)?;
		alloc_guard(new_layout.size())?;

		// No need to check for unallocated, because we are shrinking and checked that cap is < self.cap
		let ptr = {
			let Ok(old_layout) = Self::layout(self.cap) else {
				unreachable!();
			};

			debug_assert_eq!(old_layout.align(), new_layout.align());

			// SAFETY:
			// - `self.ptr` is already offset by `BUFFER_OFFSET`
			// - `old_layout` is the layout that produced `self.ptr`
			// - `new_layout.size()` is already validated
			unsafe {
				let ptr = self.buf.as_ptr().sub(Self::BUFFER_OFFSET);
				alloc::realloc(ptr, old_layout, new_layout.size())
			}
		};

		if ptr.is_null() {
			return Err(RawBufferError::AllocError { layout: new_layout });
		}

		// SAFETY:
		// - The pointer is non-null
		// - The OFFSET is included in the layout that produced the pointer
		self.buf = unsafe { NonNull::new_unchecked(ptr.add(Self::BUFFER_OFFSET)) };

		// Allocators currently return a `NonNull<[u8]>` whose length
		// matches the size requested. If that ever changes, the capacity
		// here should change to `ptr.len() / mem::size_of::<T>()`.
		self.cap = cap;
		Ok(())
	}

	// Non-inlined part of `drop`. Deallocs the bugger.
	// Safety: requires that `ptr` and `cap` is valid
	// and no more references exists to buffer.
	#[inline(never)]
	unsafe fn dealloc_slow(ptr: *mut Header, cap: usize) {
		let layout = Self::layout(cap).expect("Invalid layout");

		// SAFETY: The pointer is non-null and the layout is correct
		unsafe { alloc::dealloc(ptr as *mut u8, layout) };
	}

	/// Produces a by-ref clone for this buffer by incrementing the ref_count
	/// and returning a pointer to the same allocation.
	#[inline]
	pub fn ref_clone(&self) -> Self {
		if self.cap == 0 {
			// if cap is 0, we don't have a allocation
			// so we can just return a new danling buffer
			return Self::new();
		}

		// SAFETY: The pointer is non-null when cap is non-zero
		let ptr = unsafe { self.buf.as_ptr().sub(Self::BUFFER_OFFSET) as *mut Header };

		// SAFETY: We initialize the header when we allocate the buffer
		let header = unsafe { &*ptr };

		// Using a relaxed ordering is alright here, as knowledge of the
		// original reference prevents other threads from erroneously deleting
		// the object.
		//
		// As explained in the [Boost documentation][1], Increasing the
		// reference counter can always be done with memory_order_relaxed: New
		// references to an object can only be formed from an existing
		// reference, and passing an existing reference from one thread to
		// another must already provide any required synchronization.
		//
		// [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
		let old_size = header.ref_count.fetch_add(1, atomic::Ordering::Relaxed);

		// However we need to guard against massive refcounts in case someone
		// is `mem::forget`ing Arcs. If we don't do this the count can overflow
		// and users will use-after free. We racily saturate to `isize::MAX` on
		// the assumption that there aren't ~2 billion threads incrementing
		// the reference count at once. This branch will never be taken in
		// any realistic program.
		//
		// We abort because such a program is incredibly degenerate, and we
		// don't care to support it.
		if old_size > MAX_REFCOUNT {
			abort();
		}

		Self {
			buf: self.buf,
			cap: self.cap,
		}
	}
}

impl<const ALIGNMENT: usize> Drop for RawAlignedBuffer<ALIGNMENT> {
	#[inline]
	fn drop(&mut self) {
		if self.cap == 0 {
			// if the capacity is zero, the buffer is unallocated, so we don't need to deallocate
			return;
		}

		// SAFETY: The pointer is non-null when cap is non-zero
		let ptr = unsafe { self.buf.as_ptr().sub(Self::BUFFER_OFFSET) as *mut Header };

		// SAFETY: We initialize the header when we allocate the buffer
		let header = unsafe { &*ptr };

		// Because `fetch_sub` is already atomic, we do not need to synchronize
		// with other threads unless we are going to delete the object.
		if header.ref_count.fetch_sub(1, atomic::Ordering::Release) != 1 {
			return;
		}

		// This load is needed to prevent reordering of use of the data and
		// deletion of the data.  Because it is marked `Release`, the decreasing
		// of the reference count synchronizes with this `Acquire` load. This
		// means that use of the data happens before decreasing the reference
		// count, which happens before this load, which happens before the
		// deletion of the data.
		//
		// As explained in the [Boost documentation][1],
		//
		// [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
		header.ref_count.load(atomic::Ordering::Acquire);

		// SAFETY: Size is non-zero, and we're the last reference to the buffer
		unsafe {
			Self::dealloc_slow(ptr, self.cap);
		}
	}
}

// Central function for reserve error handling.
#[inline]
fn handle_reserve(result: Result<(), RawBufferError>) {
	match result {
		Err(RawBufferError::CapacityOverflow) => capacity_overflow(),
		Err(RawBufferError::AllocError { layout, .. }) => handle_alloc_error(layout),
		Ok(()) => { /* yay */ }
	}
}

// We need to guarantee the following:
// * We don't ever allocate `> isize::MAX` byte-size objects.
// * We don't overflow `usize::MAX` and actually allocate too little.
//
// On 64-bit we just need to check for overflow since trying to allocate
// `> isize::MAX` bytes will surely fail. On 32-bit and 16-bit we need to add
// an extra guard for this in case we're running on a platform which can use
// all 4GB in user-space, e.g., PAE or x32.

#[inline]
fn alloc_guard(alloc_size: usize) -> Result<(), RawBufferError> {
	if usize::BITS < 64 && (alloc_size - mem::size_of::<Header>() - 64) > isize::MAX as usize {
		Err(RawBufferError::CapacityOverflow)
	} else {
		Ok(())
	}
}

// One central function responsible for reporting capacity overflows. This'll
// ensure that the code generation related to these panics is minimal as there's
// only one location which panics rather than a bunch throughout the module.
#[inline(never)]
fn capacity_overflow() -> ! {
	panic!("capacity overflow");
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::alloc;

	fn validate_offset<const ALIGNMENT: usize>() {
		let expected = RawAlignedBuffer::<ALIGNMENT>::BUFFER_OFFSET;
		let header_layout = alloc::Layout::new::<Header>();

		let buffer_layout = alloc::Layout::from_size_align(1024, ALIGNMENT).expect("Invalid alignment");
		let (_, offset) = header_layout
			.extend(buffer_layout)
			.expect("Failed to exend layout");

		assert_eq!(offset, expected);
	}

	fn validate_alignment<const ALIGNMENT: usize>() {
		let layout = RawAlignedBuffer::<ALIGNMENT>::layout(1024).expect("Invalid alignment");
		assert_eq!(layout.align(), ALIGNMENT);

		let buf = RawAlignedBuffer::<ALIGNMENT>::with_capacity(1024);
		let ptr = buf.buf.as_ptr() as usize;

		assert_eq!(ptr % ALIGNMENT, 0);
	}

	#[test]
	fn offset_sanity_checks() {
		validate_offset::<8>();
		validate_offset::<16>();
		validate_offset::<32>();
		validate_offset::<64>();
		validate_offset::<128>();
		validate_offset::<256>();
	}

	#[test]
	fn alignment_check() {
		validate_alignment::<8>();
		validate_alignment::<16>();
		validate_alignment::<32>();
		validate_alignment::<64>();
		validate_alignment::<128>();
		validate_alignment::<256>();
	}
}
