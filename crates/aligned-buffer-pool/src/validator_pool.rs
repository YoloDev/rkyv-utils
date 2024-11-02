mod shared;

use self::shared::SharedValidator;
use crossbeam_queue::ArrayQueue;
use rkyv::validation::{archive::ArchiveValidator, ArchiveContext, SharedContext};
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

	pub fn validator<'a>(&self, bytes: &'a [u8]) -> PooledValidator<'a> {
		self.validator_with_max_depth(bytes, None)
	}

	pub fn validator_with_max_depth<'a>(
		&self,
		bytes: &'a [u8],
		max_depth: Option<NonZeroUsize>,
	) -> PooledValidator<'a> {
		let shared = self.inner.shared.pop().unwrap_or_default();

		PooledValidator {
			pool_ref: Arc::downgrade(&self.inner),
			archive: ArchiveValidator::with_max_depth(bytes, max_depth),
			shared,
		}
	}
}

#[derive(Debug)]
pub struct PooledValidator<'a> {
	pool_ref: Weak<Inner>,
	archive: ArchiveValidator<'a>,
	shared: SharedValidator,
}

impl<'a> Drop for PooledValidator<'a> {
	fn drop(&mut self) {
		if let Some(pool) = self.pool_ref.upgrade() {
			self.shared.clear();
			let _ = pool.shared.push(mem::take(&mut self.shared));
		}
	}
}

unsafe impl<'a, E> ArchiveContext<E> for PooledValidator<'a>
where
	ArchiveValidator<'a>: ArchiveContext<E>,
{
	#[inline]
	fn check_subtree_ptr(&mut self, ptr: *const u8, layout: &std::alloc::Layout) -> Result<(), E> {
		self.archive.check_subtree_ptr(ptr, layout)
	}

	#[inline]
	unsafe fn push_subtree_range(
		&mut self,
		root: *const u8,
		end: *const u8,
	) -> Result<Range<usize>, E> {
		self.archive.push_subtree_range(root, end)
	}

	#[inline]
	unsafe fn pop_subtree_range(&mut self, range: Range<usize>) -> Result<(), E> {
		self.archive.pop_subtree_range(range)
	}
}

impl<'a, E> SharedContext<E> for PooledValidator<'a>
where
	SharedValidator: SharedContext<E>,
{
	#[inline]
	fn start_shared(
		&mut self,
		address: usize,
		type_id: TypeId,
	) -> Result<rkyv::validation::shared::ValidationState, E> {
		self.shared.start_shared(address, type_id)
	}

	#[inline]
	fn finish_shared(&mut self, address: usize, type_id: TypeId) -> Result<(), E> {
		self.shared.finish_shared(address, type_id)
	}
}
