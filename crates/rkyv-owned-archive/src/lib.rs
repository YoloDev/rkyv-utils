use aligned_buffer::SharedAlignedBuffer;
use rkyv::{
	bytecheck::CheckBytes,
	ptr_meta::Pointee,
	rancor::{self, Strategy},
	validation::{util::access_pos_with_context, validators::DefaultValidator, ArchiveContext},
	Archive,
};
use std::{fmt, marker::PhantomData, mem, ops};

pub struct OwnedArchive<T: Archive, const ALIGNMENT: usize> {
	buffer: SharedAlignedBuffer<ALIGNMENT>,
	pos: usize,
	_phantom: PhantomData<T>,
}

impl<T: Archive, const ALIGNMENT: usize> OwnedArchive<T, ALIGNMENT> {
	#[allow(dead_code)]
	const ALIGNMENT_OK: () = assert!(mem::size_of::<T::Archived>() <= ALIGNMENT);
}

// It's safe to clone an `OwnedArchive` because the inner buffer is shared and guarantees
// that clones returns the a stable reference to the same data.
impl<T: Archive, const ALIGNMENT: usize> Clone for OwnedArchive<T, ALIGNMENT> {
	#[inline]
	fn clone(&self) -> Self {
		Self {
			buffer: self.buffer.clone(),
			pos: self.pos,
			_phantom: PhantomData,
		}
	}
}

#[cfg(feature = "bytecheck")]
impl<T: Archive, const ALIGNMENT: usize> OwnedArchive<T, ALIGNMENT> {
	pub fn new<E>(buffer: SharedAlignedBuffer<ALIGNMENT>) -> Result<Self, E>
	where
		E: rancor::Error,
		T::Archived: CheckBytes<Strategy<DefaultValidator, E>>,
	{
		let pos = buffer.len().saturating_sub(mem::size_of::<T::Archived>());
		Self::new_with_pos(buffer, pos)
	}

	pub fn new_with_pos<E>(buffer: SharedAlignedBuffer<ALIGNMENT>, pos: usize) -> Result<Self, E>
	where
		E: rancor::Error,
		T::Archived: CheckBytes<Strategy<DefaultValidator, E>>,
	{
		let mut validator = DefaultValidator::new(&buffer);
		Self::new_with_pos_and_context(buffer, pos, &mut validator)
	}

	pub fn new_with_context<C, E>(
		buffer: SharedAlignedBuffer<ALIGNMENT>,
		context: &mut C,
	) -> Result<Self, E>
	where
		T::Archived: CheckBytes<Strategy<C, E>> + Pointee<Metadata = ()>,
		C: ArchiveContext<E> + ?Sized,
		E: rancor::Error,
	{
		let pos = buffer.len().saturating_sub(mem::size_of::<T::Archived>());
		Self::new_with_pos_and_context(buffer, pos, context)
	}

	pub fn new_with_pos_and_context<C, E>(
		buffer: SharedAlignedBuffer<ALIGNMENT>,
		pos: usize,
		context: &mut C,
	) -> Result<Self, E>
	where
		T::Archived: CheckBytes<Strategy<C, E>> + Pointee<Metadata = ()>,
		C: ArchiveContext<E> + ?Sized,
		E: rancor::Error,
	{
		match access_pos_with_context::<T, C, E>(&buffer, pos, context) {
			Err(e) => Err(e),
			Ok(_) => Ok(Self {
				buffer,
				pos,
				_phantom: PhantomData,
			}),
		}
	}
}

impl<T: Archive, const ALIGNMENT: usize> OwnedArchive<T, ALIGNMENT> {
	/// # Safety
	///
	/// - The byte slice must represent an archived object.
	/// - The root of the object must be stored at the end of the slice (this is the
	///   default behavior).
	pub unsafe fn new_unchecked(buffer: SharedAlignedBuffer<ALIGNMENT>) -> Self {
		let pos = buffer.len().saturating_sub(mem::size_of::<T::Archived>());
		Self::new_unchecked_with_pos(buffer, pos)
	}

	/// # Safety
	///
	/// A `T::Archived` must be located at the given position in the byte slice.
	pub unsafe fn new_unchecked_with_pos(buffer: SharedAlignedBuffer<ALIGNMENT>, pos: usize) -> Self {
		Self {
			buffer,
			pos,
			_phantom: PhantomData,
		}
	}
}

impl<T: Archive, const ALIGNMENT: usize> ops::Deref for OwnedArchive<T, ALIGNMENT> {
	type Target = T::Archived;

	#[inline]
	fn deref(&self) -> &Self::Target {
		// SAFETY: `buffer` is required to contain a representation of T::Archived at `pos`.
		// This is checked by the safe constructors, and required by the unsafe constructors.
		unsafe { rkyv::util::access_pos_unchecked::<T>(&self.buffer, self.pos) }
	}
}

impl<T: Archive, const ALIGNMENT: usize> AsRef<T::Archived> for OwnedArchive<T, ALIGNMENT> {
	#[inline]
	fn as_ref(&self) -> &T::Archived {
		self
	}
}

impl<T: Archive, const ALIGNMENT: usize> fmt::Debug for OwnedArchive<T, ALIGNMENT>
where
	T::Archived: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(&**self, f)
	}
}

impl<T: Archive, const ALIGNMENT: usize> fmt::Display for OwnedArchive<T, ALIGNMENT>
where
	T::Archived: fmt::Display,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Display::fmt(&**self, f)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use aligned_buffer_pool::{RetainAllRetentionPolicy, SerializerPool};
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

	#[test]
	fn owned_archive() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);
		let original = TestStruct1::new("test1", 10);
		let buffer = pool
			.serialize(&original)
			.expect("failed to serialize")
			.into_shared();

		let owned_archive =
			OwnedArchive::<TestStruct1, 64>::new::<rancor::BoxedError>(buffer).expect("failed to create");

		assert_eq!(owned_archive.name, original.name);
		assert_eq!(owned_archive.boxed_name, original.boxed_name);
		assert_eq!(owned_archive.age, original.age);
	}

	#[test]
	fn clone_owned_archive() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);
		let original = TestStruct1::new("test1", 10);
		let buffer = pool
			.serialize(&original)
			.expect("failed to serialize")
			.into_shared();

		let owned_archive =
			OwnedArchive::<TestStruct1, 64>::new::<rancor::BoxedError>(buffer).expect("failed to create");

		let clone = owned_archive.clone();
		assert_eq!(owned_archive.name, clone.name);
		assert_eq!(owned_archive.boxed_name, clone.boxed_name);
		assert_eq!(owned_archive.age, clone.age);
	}
}