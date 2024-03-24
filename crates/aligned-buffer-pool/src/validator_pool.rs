mod shared;

use self::shared::SharedValidator;
use crossbeam_queue::ArrayQueue;
use rkyv::validation::{validators::ArchiveValidator, ArchiveContext, SharedContext};
use std::{
	any::TypeId,
	mem,
	num::NonZeroUsize,
	ops::Range,
	sync::{Arc, Weak},
};

#[derive(Debug)]
struct Inner {
	shared: ArrayQueue<shared::SharedValidator>,
}

#[derive(Clone, Debug)]
pub struct ValidatorPool {
	inner: Arc<Inner>,
}

impl ValidatorPool {
	pub fn new(capacity: usize) -> Self {
		Self {
			inner: Arc::new(Inner {
				shared: ArrayQueue::new(capacity),
			}),
		}
	}

	pub fn validator(&self, bytes: &[u8]) -> PooledValidator {
		self.validator_with_max_depth(bytes, None)
	}

	pub fn validator_with_max_depth(
		&self,
		bytes: &[u8],
		max_depth: Option<NonZeroUsize>,
	) -> PooledValidator {
		let shared = self.inner.shared.pop().unwrap_or_default();

		PooledValidator {
			pool_ref: Arc::downgrade(&self.inner),
			archive: ArchiveValidator::with_max_depth(bytes, max_depth),
			shared,
		}
	}
}

#[derive(Debug)]
pub struct PooledValidator {
	pool_ref: Weak<Inner>,
	archive: ArchiveValidator,
	shared: SharedValidator,
}

impl Drop for PooledValidator {
	fn drop(&mut self) {
		if let Some(pool) = self.pool_ref.upgrade() {
			self.shared.clear();
			let _ = pool.shared.push(mem::take(&mut self.shared));
		}
	}
}

unsafe impl<E> ArchiveContext<E> for PooledValidator
where
	ArchiveValidator: ArchiveContext<E>,
{
	#[inline]
	fn check_subtree_ptr(&mut self, ptr: *const u8, layout: &core::alloc::Layout) -> Result<(), E> {
		self.archive.check_subtree_ptr(ptr, layout)
	}

	#[inline]
	unsafe fn push_prefix_subtree_range(
		&mut self,
		root: *const u8,
		end: *const u8,
	) -> Result<Range<usize>, E> {
		self.archive.push_prefix_subtree_range(root, end)
	}

	#[inline]
	unsafe fn push_suffix_subtree_range(
		&mut self,
		start: *const u8,
		root: *const u8,
	) -> Result<Range<usize>, E> {
		self.archive.push_suffix_subtree_range(start, root)
	}

	#[inline]
	unsafe fn pop_subtree_range(&mut self, range: Range<usize>) -> Result<(), E> {
		unsafe { self.archive.pop_subtree_range(range) }
	}
}

impl<E> SharedContext<E> for PooledValidator
where
	SharedValidator: SharedContext<E>,
{
	#[inline]
	fn register_shared_ptr(&mut self, address: usize, type_id: TypeId) -> Result<bool, E> {
		self.shared.register_shared_ptr(address, type_id)
	}
}
