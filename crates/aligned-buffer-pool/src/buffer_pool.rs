use aligned_buffer::{
	alloc::{AllocError, Allocator, BufferAllocator, Global, RawBuffer},
	SharedAlignedBuffer, UniqueAlignedBuffer,
};
use crossbeam_queue::ArrayQueue;
use std::{
	alloc::Layout,
	mem::ManuallyDrop,
	ptr::NonNull,
	sync::{Arc, Weak},
};

pub type UniquePooledAlignedBuffer<const ALIGNMENT: usize, P, A = Global> = UniqueAlignedBuffer<
	ALIGNMENT,
	BufferPoolAllocator<P, ALIGNMENT, WeakAlignedBufferPool<P, ALIGNMENT, A>, A>,
>;

pub type SharedPooledAlignedBuffer<const ALIGNMENT: usize, P, A = Global> = SharedAlignedBuffer<
	ALIGNMENT,
	BufferPoolAllocator<P, ALIGNMENT, WeakAlignedBufferPool<P, ALIGNMENT, A>, A>,
>;

/// A policy for retaining buffers in the pool.
pub trait BufferRetentionPolicy: Clone {
	fn should_retain(&self, capaicty: usize) -> bool;
}

/// A policy that retains all buffers.
#[derive(Default, Clone, Copy)]
pub struct RetainAllRetentionPolicy;

impl BufferRetentionPolicy for RetainAllRetentionPolicy {
	#[inline(always)]
	fn should_retain(&self, _: usize) -> bool {
		true
	}
}

/// A policy that retains buffers up to a maximum size.
#[derive(Default, Clone, Copy)]
pub struct ConstMaxSizeRetentionPolicy<const SIZE: usize>;

impl<const SIZE: usize> BufferRetentionPolicy for ConstMaxSizeRetentionPolicy<SIZE> {
	#[inline(always)]
	fn should_retain(&self, capacity: usize) -> bool {
		capacity <= SIZE
	}
}

pub(crate) trait WeakAlignedBufferPoolRef<
	P: BufferRetentionPolicy,
	const ALIGNMENT: usize,
	A: Allocator + Clone,
>: Clone
{
	fn with<F>(&self, f: F)
	where
		F: FnOnce(&AlignedBufferPoolInner<P, ALIGNMENT, Self, A>);
}

pub struct BufferPoolAllocator<
	P: BufferRetentionPolicy,
	const ALIGNMENT: usize,
	R,
	A: Allocator + Clone,
> {
	policy: P,
	alloc: A,
	pool_ref: R,
}

unsafe impl<
		P: BufferRetentionPolicy,
		const ALIGNMENT: usize,
		R: WeakAlignedBufferPoolRef<P, ALIGNMENT, A>,
		A: Allocator + Clone,
	> Allocator for BufferPoolAllocator<P, ALIGNMENT, R, A>
{
	#[inline(always)]
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc.allocate(layout)
	}

	#[inline(always)]
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc.allocate_zeroed(layout)
	}

	#[inline(always)]
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		self.alloc.deallocate(ptr, layout)
	}

	#[inline(always)]
	unsafe fn grow(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc.grow(ptr, old_layout, new_layout)
	}

	#[inline(always)]
	unsafe fn grow_zeroed(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc.grow_zeroed(ptr, old_layout, new_layout)
	}

	#[inline(always)]
	unsafe fn shrink(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		self.alloc.shrink(ptr, old_layout, new_layout)
	}
}

impl<
		P: BufferRetentionPolicy,
		const ALIGNMENT: usize,
		R: WeakAlignedBufferPoolRef<P, ALIGNMENT, A>,
		A: Allocator + Clone,
	> BufferAllocator<ALIGNMENT> for BufferPoolAllocator<P, ALIGNMENT, R, A>
{
	unsafe fn deallocate_buffer(&self, raw: RawBuffer<ALIGNMENT>) {
		struct DeallocOnDrop<'a, const ALIGNMENT: usize, A: Allocator> {
			allocator: &'a A,
			raw: RawBuffer<ALIGNMENT>,
		}

		impl<'a, const ALIGNMENT: usize, A: Allocator> Drop for DeallocOnDrop<'a, ALIGNMENT, A> {
			fn drop(&mut self) {
				let (ptr, layout) = self.raw.alloc_info();
				unsafe { self.allocator.deallocate(ptr, layout) }
			}
		}

		let guard = DeallocOnDrop {
			allocator: &self.alloc,
			raw,
		};

		if self.policy.should_retain(raw.capacity().size()) {
			let alloc = self.clone();
			self.pool_ref.with(move |pool| {
				let unguard = ManuallyDrop::new(guard);
				let buf = UniqueAlignedBuffer::from_raw_parts_in(raw.buf_ptr(), 0, raw.capacity(), alloc);
				if let Err(pool) = pool.pool.push(buf) {
					// forget the pool so it doesn't get dropped
					std::mem::forget(pool);

					// then dealloc it using the guard
					drop(ManuallyDrop::into_inner(unguard));
				}
			});
		}
	}
}

impl<
		P: BufferRetentionPolicy,
		const ALIGNMENT: usize,
		R: WeakAlignedBufferPoolRef<P, ALIGNMENT, A>,
		A: Allocator + Clone,
	> Clone for BufferPoolAllocator<P, ALIGNMENT, R, A>
{
	fn clone(&self) -> Self {
		Self {
			policy: self.policy.clone(),
			alloc: self.alloc.clone(),
			pool_ref: self.pool_ref.clone(),
		}
	}
}

pub(crate) struct AlignedBufferPoolInner<
	P: BufferRetentionPolicy,
	const ALIGNMENT: usize,
	R: WeakAlignedBufferPoolRef<P, ALIGNMENT, A>,
	A: Allocator + Clone,
> {
	pool: ArrayQueue<UniqueAlignedBuffer<ALIGNMENT, BufferPoolAllocator<P, ALIGNMENT, R, A>>>,
	alloc: BufferPoolAllocator<P, ALIGNMENT, R, A>,
}

impl<
		P: BufferRetentionPolicy,
		const ALIGNMENT: usize,
		R: WeakAlignedBufferPoolRef<P, ALIGNMENT, A>,
		A: Allocator + Clone,
	> AlignedBufferPoolInner<P, ALIGNMENT, R, A>
{
	pub fn new(policy: P, alloc: A, self_ref: R, capacity: usize) -> Self {
		Self {
			pool: ArrayQueue::new(capacity),
			alloc: BufferPoolAllocator {
				policy,
				alloc,
				pool_ref: self_ref.clone(),
			},
		}
	}

	/// Gets a buffer from the pool. If the pool is empty, a new buffer is
	/// allocated and returned.
	#[inline]
	pub fn get(&self) -> UniqueAlignedBuffer<ALIGNMENT, BufferPoolAllocator<P, ALIGNMENT, R, A>> {
		if let Some(buf) = self.pool.pop() {
			buf
		} else {
			let alloc = self.alloc.clone();
			UniqueAlignedBuffer::new_in(alloc.clone())
		}
	}
}

/// A pool for allocating and recycling aligned buffers.
pub struct AlignedBufferPool<
	P: BufferRetentionPolicy,
	const ALIGNMENT: usize,
	A: Allocator + Clone = Global,
> {
	inner: Arc<AlignedBufferPoolInner<P, ALIGNMENT, WeakAlignedBufferPool<P, ALIGNMENT, A>, A>>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize> AlignedBufferPool<P, ALIGNMENT, Global> {
	pub fn new(policy: P, capacity: usize) -> Self {
		Self::new_in(policy, capacity, Global)
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize> AlignedBufferPool<P, ALIGNMENT, Global>
where
	P: Default,
{
	pub fn with_capacity(capacity: usize) -> Self {
		Self::with_capacity_in(capacity, Global)
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A: Allocator + Clone>
	AlignedBufferPool<P, ALIGNMENT, A>
where
	P: Default,
{
	pub fn with_capacity_in(capacity: usize, alloc: A) -> Self {
		Self::new_in(P::default(), capacity, alloc)
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A: Allocator + Clone>
	AlignedBufferPool<P, ALIGNMENT, A>
{
	pub fn new_in(policy: P, capacity: usize, alloc: A) -> Self {
		Self {
			inner: Arc::new_cyclic(|weak| {
				let weak = WeakAlignedBufferPool {
					inner: weak.clone(),
				};
				AlignedBufferPoolInner::new(policy, alloc, weak, capacity)
			}),
		}
	}

	/// Gets a buffer from the pool. If the pool is empty, a new buffer is
	/// allocated and returned.
	#[inline]
	pub fn get(&self) -> UniquePooledAlignedBuffer<ALIGNMENT, P, A> {
		self.inner.get()
	}

	pub fn weak(&self) -> WeakAlignedBufferPool<P, ALIGNMENT, A> {
		WeakAlignedBufferPool {
			inner: Arc::downgrade(&self.inner),
		}
	}
}

pub struct WeakAlignedBufferPool<
	P: BufferRetentionPolicy,
	const ALIGNMENT: usize,
	A: Allocator + Clone = Global,
> {
	inner: Weak<AlignedBufferPoolInner<P, ALIGNMENT, WeakAlignedBufferPool<P, ALIGNMENT, A>, A>>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A: Allocator + Clone> Clone
	for WeakAlignedBufferPool<P, ALIGNMENT, A>
{
	#[inline]
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
		}
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A: Allocator + Clone>
	WeakAlignedBufferPoolRef<P, ALIGNMENT, A> for WeakAlignedBufferPool<P, ALIGNMENT, A>
{
	fn with<F>(&self, f: F)
	where
		F: FnOnce(&AlignedBufferPoolInner<P, ALIGNMENT, Self, A>),
	{
		if let Some(inner) = self.inner.upgrade() {
			f(&inner);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_pool_reuses_buffers() {
		let pool = AlignedBufferPool::<RetainAllRetentionPolicy, 64>::with_capacity(2);
		let mut buf = pool.get();
		buf.extend([1, 2, 3]);
		drop(buf);

		let buf = pool.get();
		assert!(buf.capacity() >= 3);
	}
}
