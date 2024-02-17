use crate::{SharedAlignedBuffer, UniqueAlignedBuffer};
use rkyv::{
	boxed::{ArchivedBox, BoxResolver},
	primitive::{ArchivedUsize, FixedUsize},
	rancor::Fallible,
	ser::{Writer, WriterExt},
	Archive, Deserialize, Serialize,
};

impl<const ALIGNMENT: usize> Archive for SharedAlignedBuffer<ALIGNMENT> {
	type Archived = ArchivedBox<[u8]>;
	type Resolver = BoxResolver;

	unsafe fn resolve(&self, pos: usize, resolver: Self::Resolver, out: *mut Self::Archived) {
		let len = FixedUsize::try_from(self.len()).expect("buffer too large to archive");
		let len = ArchivedUsize::from(len);
		ArchivedBox::resolve_from_raw_parts(pos, resolver, len, out)
	}
}

impl<const ALIGNMENT: usize> Archive for UniqueAlignedBuffer<ALIGNMENT> {
	type Archived = ArchivedBox<[u8]>;
	type Resolver = BoxResolver;

	unsafe fn resolve(&self, pos: usize, resolver: Self::Resolver, out: *mut Self::Archived) {
		let len = FixedUsize::try_from(self.len()).expect("buffer too large to archive");
		let len = ArchivedUsize::from(len);
		ArchivedBox::resolve_from_raw_parts(pos, resolver, len, out)
	}
}

impl<S: Writer + Fallible, const ALIGNMENT: usize> Serialize<S> for SharedAlignedBuffer<ALIGNMENT> {
	fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
		serializer.align(ALIGNMENT)?;
		unsafe { ArchivedBox::serialize_copy_from_slice(self.as_slice(), serializer) }
	}
}

impl<S: Writer + Fallible, const ALIGNMENT: usize> Serialize<S> for UniqueAlignedBuffer<ALIGNMENT> {
	fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
		serializer.align(ALIGNMENT)?;
		unsafe { ArchivedBox::serialize_copy_from_slice(self.as_slice(), serializer) }
	}
}

impl<D: Fallible, const ALIGNMENT: usize> Deserialize<UniqueAlignedBuffer<ALIGNMENT>, D>
	for ArchivedBox<[u8]>
{
	fn deserialize(
		&self,
		_deserializer: &mut D,
	) -> Result<UniqueAlignedBuffer<ALIGNMENT>, <D as Fallible>::Error> {
		let mut buf = UniqueAlignedBuffer::with_capacity(self.len());
		buf.extend_from_slice(self);
		Ok(buf)
	}
}

impl<D: Fallible, const ALIGNMENT: usize> Deserialize<SharedAlignedBuffer<ALIGNMENT>, D>
	for ArchivedBox<[u8]>
{
	fn deserialize(
		&self,
		deserializer: &mut D,
	) -> Result<SharedAlignedBuffer<ALIGNMENT>, <D as Fallible>::Error> {
		let buf: UniqueAlignedBuffer<ALIGNMENT> = self.deserialize(deserializer)?;
		Ok(buf.into_shared())
	}
}
