use crate::pool::{Pool, PoolAllocator};
use aligned_buffer::UniqueAlignedBuffer;
use std::sync::Arc;

/// A policy for retaining buffers in the pool.
pub trait BufferRetentionPolicy<const ALIGNMENT: usize>: Copy {
	fn should_retain(&self, buffer: &UniqueAlignedBuffer<ALIGNMENT>) -> bool;
}

/// A policy that retains all buffers.
#[derive(Default, Clone, Copy)]
pub struct RetainAllRetentionPolicy;

impl<const ALIGNMENT: usize> BufferRetentionPolicy<ALIGNMENT> for RetainAllRetentionPolicy {
	#[inline(always)]
	fn should_retain(&self, _: &UniqueAlignedBuffer<ALIGNMENT>) -> bool {
		true
	}
}

/// A policy that retains buffers up to a maximum size.
#[derive(Default, Clone, Copy)]
pub struct ConstMaxSizeRetentionPolicy<const SIZE: usize>;

impl<const SIZE: usize, const ALIGNMENT: usize> BufferRetentionPolicy<ALIGNMENT>
	for ConstMaxSizeRetentionPolicy<SIZE>
{
	#[inline(always)]
	fn should_retain(&self, buffer: &UniqueAlignedBuffer<ALIGNMENT>) -> bool {
		buffer.len() <= SIZE
	}
}

#[derive(Default)]
struct BufferAllocator<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> {
	policy: P,
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> BufferAllocator<P, ALIGNMENT> {
	fn new(policy: P) -> Self {
		Self { policy }
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize>
	PoolAllocator<UniqueAlignedBuffer<ALIGNMENT>> for BufferAllocator<P, ALIGNMENT>
{
	fn allocate(&self) -> UniqueAlignedBuffer<ALIGNMENT> {
		UniqueAlignedBuffer::new()
	}

	#[inline(always)]
	fn reset(&self, buf: &mut UniqueAlignedBuffer<ALIGNMENT>) {
		buf.clear();
	}

	#[inline(always)]
	fn is_valid(&self, buf: &UniqueAlignedBuffer<ALIGNMENT>) -> bool {
		self.policy.should_retain(buf)
	}
}

pub(crate) struct AlignedBufferPoolInner<
	P: BufferRetentionPolicy<ALIGNMENT>,
	const ALIGNMENT: usize,
> {
	pool: Pool<BufferAllocator<P, ALIGNMENT>, UniqueAlignedBuffer<ALIGNMENT>>,
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize>
	AlignedBufferPoolInner<P, ALIGNMENT>
{
	pub fn new(policy: P, capacity: usize) -> Self {
		Self {
			pool: Pool::new(BufferAllocator::new(policy), capacity),
		}
	}

	/// Gets a buffer from the pool. If the pool is empty, a new buffer is
	/// allocated and returned.
	#[inline]
	pub fn get(&self) -> UniqueAlignedBuffer<ALIGNMENT> {
		self.pool.get()
	}

	/// Recycles a buffer, putting it back into the pool.
	#[inline]
	pub fn recycle(&self, buffer: UniqueAlignedBuffer<ALIGNMENT>) {
		self.pool.recycle(buffer)
	}
}

/// A pool for allocating and recycling aligned buffers.
pub struct AlignedBufferPool<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> {
	inner: Arc<AlignedBufferPoolInner<P, ALIGNMENT>>,
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> AlignedBufferPool<P, ALIGNMENT> {
	pub fn new(policy: P, capacity: usize) -> Self {
		Self {
			inner: Arc::new(AlignedBufferPoolInner::new(policy, capacity)),
		}
	}

	/// Gets a buffer from the pool. If the pool is empty, a new buffer is
	/// allocated and returned.
	#[inline]
	pub fn get(&self) -> UniqueAlignedBuffer<ALIGNMENT> {
		self.inner.get()
	}

	/// Recycles a buffer, putting it back into the pool.
	#[inline]
	pub fn recycle(&self, buffer: UniqueAlignedBuffer<ALIGNMENT>) {
		self.inner.recycle(buffer)
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> AlignedBufferPool<P, ALIGNMENT>
where
	P: Default,
{
	pub fn with_capacity(capacity: usize) -> Self {
		Self::new(P::default(), capacity)
	}
}
