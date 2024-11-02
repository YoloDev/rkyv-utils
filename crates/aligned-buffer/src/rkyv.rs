use crate::{
	alloc::{BufferAllocator, Global},
	SharedAlignedBuffer, UniqueAlignedBuffer,
};
use rkyv::{
	boxed::{ArchivedBox, BoxResolver},
	primitive::{ArchivedUsize, FixedUsize},
	rancor::Fallible,
	ser::{Positional, Writer, WriterExt},
	Archive, Deserialize, Place, Portable, Serialize,
};
use std::{convert::Infallible, ops};

#[cfg(feature = "bytecheck")]
use rkyv::{bytecheck::CheckBytes, validation::ArchiveContext};

#[derive(Debug, thiserror::Error)]
#[error("misaligned buffer")]
pub struct Misaligned;

#[repr(transparent)]
pub struct ArchivedAlignedBuffer<const ALIGNMENT: usize> {
	inner: ArchivedBox<[u8]>,
}

impl<const ALIGNMENT: usize> ArchivedAlignedBuffer<ALIGNMENT> {
	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self
	}
}

impl<const ALIGNMENT: usize> ops::Deref for ArchivedAlignedBuffer<ALIGNMENT> {
	type Target = [u8];

	#[inline]
	fn deref(&self) -> &Self::Target {
		self.inner.get()
	}
}

impl<const ALIGNMENT: usize> AsRef<[u8]> for ArchivedAlignedBuffer<ALIGNMENT> {
	#[inline]
	fn as_ref(&self) -> &[u8] {
		self
	}
}

// SAFETY: ArchivedAlignedBuffer<ALIGNMENT> is repr(transparent) over a ArchivedBox<[u8]>.
unsafe impl<const ALIGNMENT: usize> Portable for ArchivedAlignedBuffer<ALIGNMENT> {}

#[cfg(feature = "bytecheck")]
unsafe impl<C: Fallible + ?Sized, const ALIGNMENT: usize> CheckBytes<C>
	for ArchivedAlignedBuffer<ALIGNMENT>
where
	C: ArchiveContext,
	<C as Fallible>::Error: rkyv::rancor::Source,
{
	unsafe fn check_bytes(value: *const Self, context: &mut C) -> Result<(), <C as Fallible>::Error> {
		let value = value as *const ArchivedBox<[u8]>;

		// let ptr = unsafe { context.bounds_check_subtree_rel_ptr(&self.ptr)? };
		// first, check that the box is valid according to rkyv
		ArchivedBox::<[u8]>::check_bytes(value, context)?;

		// we know that the box is valid, so we can safely turn it into a reference
		let value = unsafe { &*value };

		// get the pointer to the boxed data
		let ptr = value.as_ptr();

		// check alignment
		if (ptr as usize) % ALIGNMENT != 0 {
			return Err(rkyv::rancor::Source::new(Misaligned));
		}

		Ok(())
	}
}

impl<const ALIGNMENT: usize, A> Archive for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Archived = ArchivedAlignedBuffer<ALIGNMENT>;
	type Resolver = BoxResolver;

	fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
		let len = FixedUsize::try_from(self.len()).expect("buffer too large to archive");
		let len = ArchivedUsize::from(len);
		// SAFETY: ArchivedAlignedBuffer<ALIGNMENT> is repr(transparent) over a ArchivedBox<[u8]>.
		ArchivedBox::<[u8]>::resolve_from_raw_parts(resolver, len, unsafe { out.cast_unchecked() });
	}
}

impl<const ALIGNMENT: usize, A> Archive for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Archived = ArchivedAlignedBuffer<ALIGNMENT>;
	type Resolver = BoxResolver;

	fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
		let len = FixedUsize::try_from(self.len()).expect("buffer too large to archive");
		let len = ArchivedUsize::from(len);
		// SAFETY: ArchivedAlignedBuffer<ALIGNMENT> is repr(transparent) over a ArchivedBox<[u8]>.
		ArchivedBox::<[u8]>::resolve_from_raw_parts(resolver, len, unsafe { out.cast_unchecked() });
	}
}

impl<S: Writer + Fallible, const ALIGNMENT: usize, A> Serialize<S>
	for SharedAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
		serializer.align(ALIGNMENT)?;
		let pos = serializer.pos();
		serializer.write(self.as_slice())?;
		Ok(BoxResolver::from_pos(pos))
	}
}

impl<S: Writer + Fallible, const ALIGNMENT: usize, A> Serialize<S>
	for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
		serializer.align(ALIGNMENT)?;
		let pos = serializer.pos();
		serializer.write(self.as_slice())?;
		Ok(BoxResolver::from_pos(pos))
	}
}

impl<D: Fallible, const ALIGNMENT: usize> Deserialize<UniqueAlignedBuffer<ALIGNMENT, Global>, D>
	for ArchivedAlignedBuffer<ALIGNMENT>
{
	fn deserialize(
		&self,
		_deserializer: &mut D,
	) -> Result<UniqueAlignedBuffer<ALIGNMENT>, <D as Fallible>::Error> {
		let mut buf = UniqueAlignedBuffer::with_capacity(self.inner.len());
		buf.extend_from_slice(&self.inner);
		Ok(buf)
	}
}

impl<D: Fallible, const ALIGNMENT: usize> Deserialize<SharedAlignedBuffer<ALIGNMENT, Global>, D>
	for ArchivedAlignedBuffer<ALIGNMENT>
{
	fn deserialize(
		&self,
		deserializer: &mut D,
	) -> Result<SharedAlignedBuffer<ALIGNMENT>, <D as Fallible>::Error> {
		let buf: UniqueAlignedBuffer<ALIGNMENT> = self.deserialize(deserializer)?;
		Ok(buf.into_shared())
	}
}

impl<const ALIGNMENT: usize, A> Fallible for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Error = Infallible;
}

impl<const ALIGNMENT: usize, A> Positional for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn pos(&self) -> usize {
		self.len()
	}
}

impl<E, const ALIGNMENT: usize, A> Writer<E> for UniqueAlignedBuffer<ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn write(&mut self, bytes: &[u8]) -> Result<(), E> {
		self.extend_from_slice(bytes);
		Ok(())
	}
}

#[cfg(all(test, feature = "bytecheck"))]
mod tests {
	use super::*;
	use rkyv::{
		rancor::Error,
		ser::{
			allocator::{Arena, ArenaHandle},
			sharing::Share,
			Serializer,
		},
		util::with_arena,
	};

	#[derive(Archive, Serialize, Deserialize)]
	// #[rkyv(check_bytes)]
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

	fn serializer<const ALIGNMENT: usize>(
		buffer: UniqueAlignedBuffer<ALIGNMENT>,
		arena: &mut Arena,
	) -> Serializer<UniqueAlignedBuffer<ALIGNMENT>, ArenaHandle, Share> {
		Serializer::new(buffer, arena.acquire(), Share::new())
	}

	#[test]
	fn aligned_buffer_writer() {
		with_arena(|arena| {
			let buffer = UniqueAlignedBuffer::<64>::with_capacity(1024);
			let original = TestStruct1::new("John Doe", 42);

			let mut serializer = serializer(buffer, arena);
			rkyv::api::serialize_using::<_, Error>(&original, &mut serializer)
				.expect("failed to serialize");

			let buffer = serializer.into_writer().into_shared();

			let archived = rkyv::access::<ArchivedTestStruct1, rkyv::rancor::BoxedError>(&buffer)
				.expect("failed byte-check");

			assert_eq!(archived.name, original.name);
			assert_eq!(archived.boxed_name, original.boxed_name);
			assert_eq!(archived.age, original.age);
		})
	}

	#[test]
	fn serialize_aligned_buffer_fails_if_unaligned() {
		let mut buffer = UniqueAlignedBuffer::<256>::with_capacity(100);
		for i in 0..100 {
			buffer.push(i);
		}

		let original = buffer.into_shared();
		let serialized: Result<_, rkyv::rancor::BoxedError> = rkyv::to_bytes(&original);
		let serialized = serialized.expect("failed to serialize");

		// make sure things are not aligned
		let mut vec = Vec::with_capacity(serialized.len() + 256);
		while (vec.as_ptr() as usize + vec.len()) % 16 != 0
			|| (vec.as_ptr() as usize + vec.len()) % 256 == 0
		{
			vec.push(0);
		}
		vec.extend(serialized.as_slice());

		let archived = rkyv::access::<ArchivedAlignedBuffer<256>, rkyv::rancor::BoxedError>(&vec[1..]);

		assert!(archived.is_err());
	}

	#[test]
	fn round_trip_aligned_buffer() {
		with_arena(|arena| {
			let mut buffer = UniqueAlignedBuffer::<256>::with_capacity(100);
			for i in 0..100 {
				buffer.push(i);
			}

			let original = buffer.into_shared();

			let mut serializer = serializer(UniqueAlignedBuffer::<256>::with_capacity(1024), arena);
			rkyv::api::serialize_using::<_, Error>(&original, &mut serializer)
				.expect("failed to serialize");

			let buffer = serializer.into_writer().into_shared();

			let archived = rkyv::access::<ArchivedAlignedBuffer<256>, rkyv::rancor::BoxedError>(&buffer)
				.expect("failed byte-check");

			let archived = archived.as_slice();
			let original = original.as_slice();
			assert_eq!(archived, original);
		})
	}
}
