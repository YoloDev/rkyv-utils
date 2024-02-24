use crate::{
	buffer_pool::{AlignedBufferPoolInner, BufferPoolAllocator, WeakAlignedBufferPoolRef},
	BufferRetentionPolicy,
};
use aligned_buffer::{
	alloc::{Allocator, Global},
	SharedAlignedBuffer, UniqueAlignedBuffer,
};
use crossbeam_queue::ArrayQueue;
use fxhash::FxHashMap;
use std::{
	alloc,
	collections::hash_map,
	fmt, mem,
	ptr::{self, NonNull},
	sync::{Arc, Weak},
};

pub type SerializerAlignedBuffer<const ALIGNMENT: usize, P, A = Global> = SharedAlignedBuffer<
	ALIGNMENT,
	BufferPoolAllocator<P, ALIGNMENT, SerializerWeakRef<P, ALIGNMENT, A>, A>,
>;

struct RentedUnify<P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
where
	A: Allocator + Clone,
{
	owner: Weak<Inner<P, ALIGNMENT, A>>,
	map: FxHashMap<usize, usize>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> RentedUnify<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	fn new(owner: &Arc<Inner<P, ALIGNMENT, A>>, map: FxHashMap<usize, usize>) -> Self {
		Self {
			owner: Arc::downgrade(owner),
			map,
		}
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> Drop for RentedUnify<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	fn drop(&mut self) {
		if let Some(owner) = self.owner.upgrade() {
			let _ = owner.unify.push(std::mem::take(&mut self.map));
		}
	}
}

struct Inner<P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
where
	A: Allocator + Clone,
{
	writers: AlignedBufferPoolInner<P, ALIGNMENT, SerializerWeakRef<P, ALIGNMENT, A>, A>,
	scratch: AlignedBufferPoolInner<P, ALIGNMENT, SerializerScratchRef<P, ALIGNMENT, A>, A>,
	unify: ArrayQueue<FxHashMap<usize, usize>>,
}

pub struct SerializerPool<P: BufferRetentionPolicy, const ALIGNMENT: usize, A = Global>
where
	A: Allocator + Clone,
{
	inner: Arc<Inner<P, ALIGNMENT, A>>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize> SerializerPool<P, ALIGNMENT> {
	pub fn new(policy: P, capacity: usize) -> Self {
		Self::new_in(policy, capacity, Global)
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> SerializerPool<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	pub fn new_in(policy: P, capacity: usize, alloc: A) -> Self {
		let inner = Arc::new_cyclic(|weak| {
			let writer_ref = SerializerWeakRef {
				inner: weak.clone(),
			};
			let scratch_ref = SerializerScratchRef {
				inner: weak.clone(),
			};

			let writers =
				AlignedBufferPoolInner::new(policy.clone(), alloc.clone(), writer_ref, capacity);
			let scratch =
				AlignedBufferPoolInner::new(policy.clone(), alloc.clone(), scratch_ref, capacity);
			let unify = ArrayQueue::new(capacity);

			Inner {
				writers,
				scratch,
				unify,
			}
		});

		Self { inner }
	}

	pub fn get(&self) -> Serializer<P, ALIGNMENT, A> {
		let writer = self.inner.writers.get();
		let scratch = self.inner.scratch.get();
		let unify = self.inner.unify.pop().unwrap_or_default();

		Serializer {
			writer,
			scratch,
			unify: RentedUnify::new(&self.inner, unify),
		}
	}

	pub fn serialize<
		T: rkyv::Serialize<rkyv::rancor::Strategy<Serializer<P, ALIGNMENT, A>, rkyv::rancor::BoxedError>>,
	>(
		&self,
		value: &T,
	) -> Result<SerializerAlignedBuffer<ALIGNMENT, P, A>, rkyv::rancor::BoxedError> {
		let mut buf = self.get();
		buf.serialize(value)?;
		Ok(buf.into_buffer())
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize> SerializerPool<P, ALIGNMENT>
where
	P: Default,
{
	pub fn with_capacity(capacity: usize) -> Self {
		Self::new(P::default(), capacity)
	}
}

pub struct Serializer<P: BufferRetentionPolicy, const ALIGNMENT: usize, A = Global>
where
	A: Allocator + Clone,
{
	writer: UniqueAlignedBuffer<
		ALIGNMENT,
		BufferPoolAllocator<P, ALIGNMENT, SerializerWeakRef<P, ALIGNMENT, A>, A>,
	>,
	scratch: UniqueAlignedBuffer<
		ALIGNMENT,
		BufferPoolAllocator<P, ALIGNMENT, SerializerScratchRef<P, ALIGNMENT, A>, A>,
	>,
	unify: RentedUnify<P, ALIGNMENT, A>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> Serializer<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	pub fn into_buffer(self) -> SerializerAlignedBuffer<ALIGNMENT, P, A> {
		self.writer.into_shared()
	}

	pub fn serialize<T: rkyv::Serialize<rkyv::rancor::Strategy<Self, rkyv::rancor::BoxedError>>>(
		&mut self,
		value: &T,
	) -> Result<(), rkyv::rancor::BoxedError> {
		rkyv::util::serialize(value, self)
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> From<Serializer<P, ALIGNMENT, A>>
	for SerializerAlignedBuffer<ALIGNMENT, P, A>
where
	A: Allocator + Clone,
{
	#[inline]
	fn from(serializer: Serializer<P, ALIGNMENT, A>) -> Self {
		serializer.into_buffer()
	}
}

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
	rkyv::ser::sharing::Sharing<E> for Serializer<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	#[inline]
	fn get_shared_ptr(&self, address: usize) -> Option<usize> {
		self.unify.map.get(&address).copied()
	}

	fn add_shared_ptr(&mut self, address: usize, pos: usize) -> Result<(), E> {
		match self.unify.map.entry(address) {
			hash_map::Entry::Occupied(_) => {
				rkyv::rancor::fail!(DuplicateSharedPointer { address });
			}
			hash_map::Entry::Vacant(e) => {
				e.insert(pos);
				Ok(())
			}
		}
	}
}

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
	rkyv::ser::Allocator<E> for Serializer<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	#[inline]
	unsafe fn push_alloc(&mut self, layout: alloc::Layout) -> Result<ptr::NonNull<[u8]>, E> {
		let scratch: &mut UniqueAlignedBuffer<ALIGNMENT, _> = &mut self.scratch;
		let alloc_offset = scratch.as_ptr() as usize + scratch.len();
		let alloc_pad = 0usize.wrapping_sub(alloc_offset) % layout.align();
		let alloc_len = alloc_pad + layout.size();
		scratch.reserve(alloc_len);

		// we need to make sure the bytes are initialized, as `UniqueAlignedBuffer` allows creating a slice
		// form 0 to `len` - and assumes that the bytes are initialized.
		let ptr = scratch.as_mut_ptr().wrapping_add(scratch.len());
		ptr.write_bytes(0u8, alloc_len);

		let old_len = scratch.len();
		let start = old_len + alloc_pad;
		scratch.set_len(scratch.len() + alloc_len);

		let result_slice =
			ptr::slice_from_raw_parts_mut(scratch.as_mut_ptr().wrapping_add(start), layout.size());

		debug_assert!((*result_slice).iter().all(|&b| b == 0));
		Ok(NonNull::new_unchecked(result_slice))
	}

	#[inline]
	unsafe fn pop_alloc(&mut self, ptr: ptr::NonNull<u8>, layout: alloc::Layout) -> Result<(), E> {
		let scratch: &mut UniqueAlignedBuffer<ALIGNMENT, _> = &mut self.scratch;

		let bytes = scratch.as_mut();
		let ptr = ptr.as_ptr();

		if bytes.as_mut_ptr_range().contains(&ptr) {
			let popped_pos = ptr.offset_from(bytes.as_mut_ptr()) as usize;
			if popped_pos + layout.size() <= scratch.len() {
				scratch.set_len(popped_pos);
				Ok(())
			} else {
				rkyv::rancor::fail!(BufferAllocError::NotPoppedInReverseOrder {
					pos: scratch.len(),
					popped_pos,
					popped_size: layout.size(),
				});
			}
		} else {
			rkyv::rancor::fail!(BufferAllocError::DoesNotContainAllocation);
		}
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> rkyv::ser::Positional
	for Serializer<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	#[inline(always)]
	fn pos(&self) -> usize {
		rkyv::ser::Positional::pos(&self.writer)
	}
}

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
	rkyv::ser::Writer<E> for Serializer<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	#[inline(always)]
	fn write(&mut self, bytes: &[u8]) -> Result<(), E> {
		rkyv::ser::Writer::write(&mut self.writer, bytes).map_err(E::new)
	}
}

#[derive(Debug)]
struct DuplicateSharedPointer {
	address: usize,
}

impl fmt::Display for DuplicateSharedPointer {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"duplicate shared pointer: {:#.*x}",
			mem::size_of::<usize>() * 2,
			self.address
		)
	}
}

impl std::error::Error for DuplicateSharedPointer {}

#[derive(Debug)]
enum BufferAllocError {
	NotPoppedInReverseOrder {
		pos: usize,
		popped_pos: usize,
		popped_size: usize,
	},
	DoesNotContainAllocation,
}

impl fmt::Display for BufferAllocError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NotPoppedInReverseOrder {
				pos,
				popped_pos,
				popped_size,
			} => write!(
				f,
				"allocation popped at {} with length {} runs past buffer allocator start {}",
				popped_pos, popped_size, pos,
			),
			Self::DoesNotContainAllocation => {
				write!(f, "allocator does not contain popped allocation")
			}
		}
	}
}

impl std::error::Error for BufferAllocError {}

struct SerializerScratchRef<P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
where
	A: Allocator + Clone,
{
	inner: Weak<Inner<P, ALIGNMENT, A>>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> Clone
	for SerializerScratchRef<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
		}
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A: Allocator + Clone>
	WeakAlignedBufferPoolRef<P, ALIGNMENT, A> for SerializerScratchRef<P, ALIGNMENT, A>
{
	fn with<F>(&self, f: F)
	where
		F: FnOnce(&AlignedBufferPoolInner<P, ALIGNMENT, Self, A>),
	{
		if let Some(inner) = self.inner.upgrade() {
			f(&inner.scratch);
		}
	}
}

pub struct SerializerWeakRef<P: BufferRetentionPolicy, const ALIGNMENT: usize, A>
where
	A: Allocator + Clone,
{
	inner: Weak<Inner<P, ALIGNMENT, A>>,
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> Clone
	for SerializerWeakRef<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
		}
	}
}

impl<P: BufferRetentionPolicy, const ALIGNMENT: usize, A> WeakAlignedBufferPoolRef<P, ALIGNMENT, A>
	for SerializerWeakRef<P, ALIGNMENT, A>
where
	A: Allocator + Clone,
{
	fn with<F>(&self, f: F)
	where
		F: FnOnce(&AlignedBufferPoolInner<P, ALIGNMENT, Self, A>),
	{
		if let Some(inner) = self.inner.upgrade() {
			f(&inner.writers);
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::RetainAllRetentionPolicy;

	use super::*;
	use rkyv::{Archive, Deserialize, Serialize};

	#[derive(Archive, Serialize, Deserialize)]
	#[archive(check_bytes)]
	struct TestStruct1 {
		name: String,
		boxed_name: Box<str>,
		age: u16,
	}

	impl TestStruct1 {
		fn new(name: &str, age: u16) -> Self {
			Self {
				name: name.to_string(),
				boxed_name: name.into(),
				age,
			}
		}
	}

	fn check(buf: &[u8], value: &TestStruct1) {
		let archived =
			rkyv::access::<TestStruct1, rkyv::rancor::BoxedError>(buf).expect("failed to access");

		assert_eq!(archived.name, value.name);
		assert_eq!(archived.boxed_name, value.boxed_name);
		assert_eq!(archived.age, value.age);
	}

	#[test]
	fn serialization_buffer() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);
		let test1 = TestStruct1::new("test1", 10);
		let test2 = TestStruct1::new("test2", 20);

		let buf1 = pool.serialize(&test1).expect("failed to serialize");
		check(&buf1, &test1);
		drop(buf1);

		let buf2 = pool.serialize(&test2).expect("failed to serialize");
		check(&buf2, &test2);
		drop(buf2);
	}

	#[test]
	fn discard_buffers() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);

		for i in 0..20 {
			let test = TestStruct1::new(&format!("Test Name {}", i), i * 10);
			let buf = pool.serialize(&test).expect("failed to serialize");
			check(&buf, &test);
		}
	}
}
