use crate::{
	buffer_pool::AlignedBufferPoolInner,
	pool::{Pool, PoolAllocator},
	BufferRetentionPolicy,
};
use aligned_buffer::{SharedAlignedBuffer, UniqueAlignedBuffer};
use fxhash::FxHashMap;
use std::{
	alloc,
	collections::hash_map,
	fmt, mem,
	ptr::{self, NonNull},
	sync::{Arc, Weak},
};

struct FxUnify {
	map: FxHashMap<usize, usize>,
}

impl FxUnify {
	fn new() -> Self {
		Self {
			map: FxHashMap::default(),
		}
	}
}

struct SharingAllocator;

impl PoolAllocator<FxUnify> for SharingAllocator {
	fn allocate(&self) -> FxUnify {
		FxUnify {
			map: FxHashMap::default(),
		}
	}

	#[inline(always)]
	fn is_valid(&self, unify: &FxUnify) -> bool {
		unify.map.capacity() < 256
	}

	#[inline(always)]
	fn reset(&self, unify: &mut FxUnify) {
		unify.map.clear();
	}
}

struct Inner<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> {
	writers: AlignedBufferPoolInner<P, ALIGNMENT>,
	scratch: AlignedBufferPoolInner<P, ALIGNMENT>,
	unify: Pool<SharingAllocator, FxUnify>,
}

pub struct SerializerPool<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> {
	inner: Arc<Inner<P, ALIGNMENT>>,
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> SerializerPool<P, ALIGNMENT> {
	pub fn new(policy: P, capacity: usize) -> Self {
		let writers = AlignedBufferPoolInner::new(policy, capacity);
		let scratch = AlignedBufferPoolInner::new(policy, capacity);
		let unify_pool = Pool::new(SharingAllocator, capacity);

		Self {
			inner: Arc::new(Inner {
				writers,
				scratch,
				unify: unify_pool,
			}),
		}
	}

	pub fn get(&self) -> Serializer<P, ALIGNMENT> {
		let writer = self.inner.writers.get();
		let scratch = self.inner.scratch.get();
		let unify = self.inner.unify.get();

		Serializer {
			pool: Arc::downgrade(&self.inner),
			writer,
			scratch,
			unify,
		}
	}

	pub fn recycle_buffer(&self, buffer: UniqueAlignedBuffer<ALIGNMENT>) {
		self.inner.writers.recycle(buffer);
	}

	pub fn serialize<
		T: rkyv::Serialize<rkyv::rancor::Strategy<Serializer<P, ALIGNMENT>, rkyv::rancor::BoxedError>>,
	>(
		&self,
		value: &T,
	) -> Result<UniqueAlignedBuffer<ALIGNMENT>, rkyv::rancor::BoxedError> {
		let mut buf = self.get();
		buf.serialize(value)?;
		Ok(buf.into_buffer())
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> SerializerPool<P, ALIGNMENT>
where
	P: Default,
{
	pub fn with_capacity(capacity: usize) -> Self {
		Self::new(P::default(), capacity)
	}
}

pub struct Serializer<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> {
	pool: Weak<Inner<P, ALIGNMENT>>,
	writer: UniqueAlignedBuffer<ALIGNMENT>,
	scratch: UniqueAlignedBuffer<ALIGNMENT>,
	unify: FxUnify,
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> Drop
	for Serializer<P, ALIGNMENT>
{
	fn drop(&mut self) {
		if let Some(pool) = self.pool.upgrade() {
			let writer = mem::replace(&mut self.writer, UniqueAlignedBuffer::new());
			let scratch = mem::replace(&mut self.scratch, UniqueAlignedBuffer::new());
			let unify = mem::replace(&mut self.unify, FxUnify::new());

			pool.writers.recycle(writer);
			pool.scratch.recycle(scratch);
			pool.unify.recycle(unify);
		}
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> Serializer<P, ALIGNMENT> {
	pub fn into_buffer(mut self) -> UniqueAlignedBuffer<ALIGNMENT> {
		if let Some(pool) = self.pool.upgrade() {
			let scratch = mem::replace(&mut self.scratch, UniqueAlignedBuffer::new());
			let unify = mem::replace(&mut self.unify, FxUnify::new());

			pool.scratch.recycle(scratch);
			pool.unify.recycle(unify);
		}

		// prevent the new values from being inserted into the pool
		self.pool = Weak::new();
		mem::replace(&mut self.writer, UniqueAlignedBuffer::new())
	}

	pub fn serialize<T: rkyv::Serialize<rkyv::rancor::Strategy<Self, rkyv::rancor::BoxedError>>>(
		&mut self,
		value: &T,
	) -> Result<(), rkyv::rancor::BoxedError> {
		rkyv::util::serialize(value, self)
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> From<Serializer<P, ALIGNMENT>>
	for UniqueAlignedBuffer<ALIGNMENT>
{
	#[inline]
	fn from(serializer: Serializer<P, ALIGNMENT>) -> Self {
		serializer.into_buffer()
	}
}

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> From<Serializer<P, ALIGNMENT>>
	for SharedAlignedBuffer<ALIGNMENT>
{
	#[inline]
	fn from(serializer: Serializer<P, ALIGNMENT>) -> Self {
		serializer.into_buffer().into_shared()
	}
}

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize>
	rkyv::ser::sharing::Sharing<E> for Serializer<P, ALIGNMENT>
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

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize>
	rkyv::ser::Allocator<E> for Serializer<P, ALIGNMENT>
{
	#[inline]
	unsafe fn push_alloc(&mut self, layout: alloc::Layout) -> Result<ptr::NonNull<[u8]>, E> {
		let scratch: &mut UniqueAlignedBuffer<ALIGNMENT> = &mut self.scratch;
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
		let scratch: &mut UniqueAlignedBuffer<ALIGNMENT> = &mut self.scratch;

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

impl<P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize> rkyv::ser::Positional
	for Serializer<P, ALIGNMENT>
{
	#[inline(always)]
	fn pos(&self) -> usize {
		rkyv::ser::Positional::pos(&self.writer)
	}
}

impl<E: rkyv::rancor::Error, P: BufferRetentionPolicy<ALIGNMENT>, const ALIGNMENT: usize>
	rkyv::ser::Writer<E> for Serializer<P, ALIGNMENT>
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

		let buf2 = pool.serialize(&test2).expect("failed to serialize");
		check(&buf2, &test2);

		pool.recycle_buffer(buf1);
		pool.recycle_buffer(buf2);
	}

	#[test]
	fn reuse_buffer() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);

		let test1 = TestStruct1::new("Test Name 1", 10);
		let buf = pool.serialize(&test1).expect("failed to serialize");
		check(&buf, &test1);
		let initial_cap = buf.capacity();
		pool.recycle_buffer(buf);

		let test2 = TestStruct1::new("Test Name 2", 20);
		let buf = pool.serialize(&test2).expect("failed to serialize");
		check(&buf, &test2);
		pool.recycle_buffer(buf);

		let test3 = TestStruct1::new("Test Name 3", 30);
		let buf = pool.serialize(&test3).expect("failed to serialize");
		check(&buf, &test3);
		pool.recycle_buffer(buf);

		let test4 = TestStruct1::new("Test Name 4", 40);
		let buf = pool.serialize(&test4).expect("failed to serialize");
		check(&buf, &test4);
		debug_assert_eq!(buf.capacity(), initial_cap);
		pool.recycle_buffer(buf);
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
