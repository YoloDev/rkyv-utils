mod layout;

use self::layout::LayoutHelper;
use std::{
	alloc::{self, handle_alloc_error, Layout},
	cmp, mem,
	ptr::NonNull,
	sync::atomic,
};

#[derive(Debug, thiserror::Error)]
pub enum RawBufferError {
	#[error("capacity overflow")]
	CapacityOverflow,

	#[non_exhaustive]
	#[error("allocation error")]
	AllocError { layout: Layout },
}

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
		if size == 0 {
			return Self::new();
		}

		let layout = match Self::layout(size) {
			Ok(layout) => layout,
			Err(_) => capacity_overflow(),
		};

		match alloc_guard(layout.size()) {
			Ok(_) => {}
			Err(_) => capacity_overflow(),
		}

		// SAFETY:
		// - The layout is not zero-sized, even if `size` is zero, because we extend it from the header layout.
		let ptr = unsafe { alloc::alloc(layout) };

		if ptr.is_null() {
			handle_alloc_error(layout);
		}

		// SAFETY:
		// - The pointer is non-null
		// - The OFFSET is included in the layout that produced the pointer
		let buf = unsafe { NonNull::new_unchecked(ptr.add(Self::BUFFER_OFFSET)) };

		// Allocators currently return a `NonNull<[u8]>` whose length
		// matches the size requested. If that ever changes, the capacity
		// here should change to `ptr.len() / mem::size_of::<T>()`.
		let cap = size;

		Self { buf, cap }
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
	#[inline]
	pub fn reserve(&mut self, len: usize, additional: usize) {
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
	pub fn try_reserve(&mut self, len: usize, additional: usize) -> Result<(), RawBufferError> {
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
	pub fn reserve_exact(&mut self, len: usize, additional: usize) {
		handle_reserve(self.try_reserve_exact(len, additional));
	}

	/// The same as `reserve_exact`, but returns on errors instead of panicking or aborting.
	pub fn try_reserve_exact(&mut self, len: usize, additional: usize) -> Result<(), RawBufferError> {
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
	pub fn shrink_to_fit(&mut self, cap: usize) {
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
		let new_layout = Self::layout(cap)?;
		alloc_guard(new_layout.size())?;

		let ptr = if self.cap == 0 {
			// no allocation to resize, allocate anew
			unsafe { alloc::alloc(new_layout) }
		} else {
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

		let ptr = if self.cap == 0 {
			// no allocation to resize, allocate anew
			unsafe { alloc::alloc(new_layout) }
		} else {
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
}

impl<const ALIGNMENT: usize> Drop for RawAlignedBuffer<ALIGNMENT> {
	fn drop(&mut self) {
		if self.cap == 0 {
			// if the capacity is zero, the buffer is unallocated, so we don't need to deallocate
			return;
		}

		let layout = Self::layout(self.cap).expect("Invalid layout");

		// SAFETY: The pointer is non-null and the layout is correct
		unsafe { alloc::dealloc(self.buf.as_ptr().sub(Self::BUFFER_OFFSET), layout) };
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
