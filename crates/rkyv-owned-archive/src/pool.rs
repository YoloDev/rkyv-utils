use crate::OwnedArchive;
use aligned_buffer::{
	alloc::{BufferAllocator, Global},
	SharedAlignedBuffer, DEFAULT_BUFFER_ALIGNMENT,
};
use aligned_buffer_pool::{PooledValidator, SerializerPoolAllocator, ValidatorPool};
use rkyv::{
	api::access_pos_with_context,
	bytecheck::CheckBytes,
	rancor::{self, Strategy},
	Portable,
};
use std::mem;

pub type PooledArchive<T, P, const ALIGNMENT: usize = DEFAULT_BUFFER_ALIGNMENT, A = Global> =
	OwnedArchive<T, ALIGNMENT, SerializerPoolAllocator<P, ALIGNMENT, A>>;

#[cfg(feature = "bytecheck")]
impl<T: Portable, const ALIGNMENT: usize, A> OwnedArchive<T, ALIGNMENT, A>
where
	A: BufferAllocator<ALIGNMENT>,
{
	pub fn new_with_pooled_validator<E>(
		buffer: SharedAlignedBuffer<ALIGNMENT, A>,
		validator_pool: &ValidatorPool,
	) -> Result<Self, E>
	where
		E: rancor::Source,
		T: for<'a> CheckBytes<Strategy<PooledValidator<'a>, E>>,
	{
		let pos = buffer.len().saturating_sub(mem::size_of::<T>());
		Self::new_with_pos_and_pooled_validator(buffer, pos, validator_pool)
	}

	pub fn new_with_pos_and_pooled_validator<E>(
		buffer: SharedAlignedBuffer<ALIGNMENT, A>,
		pos: usize,
		validator_pool: &ValidatorPool,
	) -> Result<Self, E>
	where
		E: rancor::Source,
		T: for<'a> CheckBytes<Strategy<PooledValidator<'a>, E>>,
	{
		{
			let mut validator = validator_pool.validator(&buffer);
			access_pos_with_context::<T, _, E>(&buffer, pos, &mut validator)?;
		}

		// SAFETY: We just checked that the buffer is valid
		let result = unsafe { Self::new_unchecked_with_pos(buffer, pos) };
		Ok(result)
	}
}
