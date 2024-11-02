#[cfg(feature = "pool")]
mod pool;

use aligned_buffer::{
	alloc::{BufferAllocator, Global},
	SharedAlignedBuffer, DEFAULT_BUFFER_ALIGNMENT,
};
use rkyv::Portable;
use std::{fmt, marker::PhantomData, mem, ops};

#[cfg(feature = "bytecheck")]
use rkyv::{
	api::{access_pos_with_context, high::HighValidator},
	bytecheck::CheckBytes,
	ptr_meta::Pointee,
	rancor::{self, Strategy},
	validation::{archive::ArchiveValidator, shared::SharedValidator, ArchiveContext, Validator},
};

#[cfg(feature = "pool")]
pub use pool::PooledArchive;

#[cfg(feature = "pool")]
pub use aligned_buffer_pool;

pub use aligned_buffer;

pub struct OwnedArchive<T: Portable, const ALIGNMENT: usize = DEFAULT_BUFFER_ALIGNMENT, A = Global>
where
	A: BufferAllocator<ALIGNMENT>,
{
	buffer: SharedAlignedBuffer<ALIGNMENT, A>,
	pos: usize,
	_phantom: PhantomData<T>,
}

impl<T: Portable, const ALIGNMENT: usize, A> OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[allow(dead_code)]
	const ALIGNMENT_OK: () = assert!(mem::size_of::<T>() <= ALIGNMENT);
}

// It's safe to clone an `OwnedArchive` because the inner buffer is shared and guarantees
// that clones returns the a stable reference to the same data.
impl<T: Portable, const ALIGNMENT: usize, A> Clone for OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT> + Clone,
{
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
impl<T: Portable, const ALIGNMENT: usize, A> OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	pub fn new<E>(buffer: SharedAlignedBuffer<ALIGNMENT, A>) -> Result<Self, E>
	where
		E: rancor::Source,
		T: for<'a> CheckBytes<HighValidator<'a, E>>,
	{
		let pos = buffer.len().saturating_sub(mem::size_of::<T>());
		Self::new_with_pos(buffer, pos)
	}

	pub fn new_with_pos<E>(buffer: SharedAlignedBuffer<ALIGNMENT, A>, pos: usize) -> Result<Self, E>
	where
		E: rancor::Source,
		T: for<'a> CheckBytes<HighValidator<'a, E>>,
	{
		let mut validator = Validator::new(ArchiveValidator::new(&buffer), SharedValidator::new());
		match access_pos_with_context::<T, _, E>(&buffer, pos, &mut validator) {
			Err(e) => Err(e),
			Ok(_) => Ok(Self {
				buffer,
				pos,
				_phantom: PhantomData,
			}),
		}
	}

	pub fn new_with_context<C, E>(
		buffer: SharedAlignedBuffer<ALIGNMENT, A>,
		context: &mut C,
	) -> Result<Self, E>
	where
		T: CheckBytes<Strategy<C, E>> + Pointee<Metadata = ()>,
		C: ArchiveContext<E> + ?Sized,
		E: rancor::Source,
	{
		let pos = buffer.len().saturating_sub(mem::size_of::<T>());
		Self::new_with_pos_and_context(buffer, pos, context)
	}

	pub fn new_with_pos_and_context<C, E>(
		buffer: SharedAlignedBuffer<ALIGNMENT, A>,
		pos: usize,
		context: &mut C,
	) -> Result<Self, E>
	where
		T: CheckBytes<Strategy<C, E>> + Pointee<Metadata = ()>,
		C: ArchiveContext<E> + ?Sized,
		E: rancor::Source,
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

	pub fn map<U: Portable, F>(self, f: F) -> OwnedArchive<U, ALIGNMENT, A>
	where
		F: for<'a> FnOnce(&'a T) -> &'a U,
	{
		self.map_with_buffer(|a, _| f(a))
	}

	pub fn map_with_buffer<U: Portable, F>(self, f: F) -> OwnedArchive<U, ALIGNMENT, A>
	where
		F: for<'a> FnOnce(&'a T, &'a [u8]) -> &'a U,
	{
		let ptr_start = f(&*self, &self.buffer) as *const U as usize;
		let ptr_end = ptr_start + mem::size_of::<U>();
		let buf_start = self.buffer.as_ptr() as usize;
		let buf_end = buf_start + self.buffer.len();

		// check that U is within the bounds of the buffer
		assert!((buf_start..=buf_end).contains(&ptr_start));
		assert!((buf_start..=buf_end).contains(&ptr_end));
		let pos = ptr_start - buf_start;

		// SAFETY: U is within the bounds of the buffer
		unsafe { OwnedArchive::new_unchecked_with_pos(self.buffer, pos) }
	}

	pub fn try_map<U: Portable, E, F>(self, f: F) -> Result<OwnedArchive<U, ALIGNMENT, A>, E>
	where
		F: for<'a> FnOnce(&'a T) -> Result<&'a U, E>,
	{
		self.try_map_with_buffer(|a, _| f(a))
	}

	pub fn try_map_with_buffer<U: Portable, E, F>(
		self,
		f: F,
	) -> Result<OwnedArchive<U, ALIGNMENT, A>, E>
	where
		F: for<'a> FnOnce(&'a T, &'a [u8]) -> Result<&'a U, E>,
	{
		let ptr_start = f(&*self, &self.buffer)? as *const U as usize;
		let ptr_end = ptr_start + mem::size_of::<U>();
		let buf_start = self.buffer.as_ptr() as usize;
		let buf_end = buf_start + self.buffer.len();

		// check that U is within the bounds of the buffer
		assert!((buf_start..=buf_end).contains(&ptr_start));
		assert!((buf_start..=buf_end).contains(&ptr_end));
		let pos = ptr_start - buf_start;

		// SAFETY: U is within the bounds of the buffer
		Ok(unsafe { OwnedArchive::new_unchecked_with_pos(self.buffer, pos) })
	}
}

impl<T: Portable, const ALIGNMENT: usize, A> OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	/// # Safety
	///
	/// - The byte slice must represent an archived object.
	/// - The root of the object must be stored at the end of the slice (this is the
	///   default behavior).
	pub unsafe fn new_unchecked(buffer: SharedAlignedBuffer<ALIGNMENT, A>) -> Self {
		let pos = buffer.len().saturating_sub(mem::size_of::<T>());
		Self::new_unchecked_with_pos(buffer, pos)
	}

	/// # Safety
	///
	/// A `T::Archived` must be located at the given position in the byte slice.
	pub unsafe fn new_unchecked_with_pos(
		buffer: SharedAlignedBuffer<ALIGNMENT, A>,
		pos: usize,
	) -> Self {
		Self {
			buffer,
			pos,
			_phantom: PhantomData,
		}
	}
}

impl<T: Portable, const ALIGNMENT: usize, A> ops::Deref for OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	type Target = T;

	#[inline]
	fn deref(&self) -> &Self::Target {
		// SAFETY: `buffer` is required to contain a representation of T::Archived at `pos`.
		// This is checked by the safe constructors, and required by the unsafe constructors.
		unsafe { rkyv::api::access_pos_unchecked::<T>(&self.buffer, self.pos) }
	}
}

impl<T: Portable, const ALIGNMENT: usize, A> AsRef<T> for OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	#[inline]
	fn as_ref(&self) -> &T {
		self
	}
}

impl<T: Portable, const ALIGNMENT: usize, A> fmt::Debug for OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
	T: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(&**self, f)
	}
}

impl<T: Portable, const ALIGNMENT: usize, A> fmt::Display for OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
	T: fmt::Display,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Display::fmt(&**self, f)
	}
}

#[cfg(all(test, feature = "bytecheck"))]
mod tests {
	use super::*;
	use aligned_buffer_pool::{RetainAllRetentionPolicy, SerializerPool};
	use rkyv::{Archive, Deserialize, Serialize};

	#[derive(Archive, Serialize, Deserialize)]
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
		let buffer = pool.serialize(&original).expect("failed to serialize");

		let owned_archive =
			OwnedArchive::<ArchivedTestStruct1, 64, _>::new::<rancor::BoxedError>(buffer)
				.expect("failed to create");

		assert_eq!(owned_archive.name, original.name);
		assert_eq!(owned_archive.boxed_name, original.boxed_name);
		assert_eq!(owned_archive.age, original.age);
	}

	#[test]
	fn clone_owned_archive() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);
		let original = TestStruct1::new("test1", 10);
		let buffer = pool.serialize(&original).expect("failed to serialize");

		let owned_archive =
			OwnedArchive::<ArchivedTestStruct1, 64, _>::new::<rancor::BoxedError>(buffer)
				.expect("failed to create");

		let clone = owned_archive.clone();
		assert_eq!(owned_archive.name, clone.name);
		assert_eq!(owned_archive.boxed_name, clone.boxed_name);
		assert_eq!(owned_archive.age, clone.age);
	}

	#[test]
	fn mapped_owned_archive() {
		let pool = SerializerPool::<RetainAllRetentionPolicy, 64>::with_capacity(10);
		let original = TestStruct1::new("test1", 10);
		let buffer = pool.serialize(&original).expect("failed to serialize");

		let owned_archive =
			OwnedArchive::<ArchivedTestStruct1, 64, _>::new::<rancor::BoxedError>(buffer)
				.expect("failed to create");

		let mapped = owned_archive.map(|a| &a.name);
		assert_eq!(*mapped, original.name);
	}
}
