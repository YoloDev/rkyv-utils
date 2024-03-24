use crate::OwnedArchive;
use aligned_buffer::{alloc::Global, DEFAULT_BUFFER_ALIGNMENT};
use aligned_buffer_pool::SerializerPoolAllocator;

pub type PooledArchive<T, P, const ALIGNMENT: usize = DEFAULT_BUFFER_ALIGNMENT, A = Global> =
	OwnedArchive<T, ALIGNMENT, SerializerPoolAllocator<P, ALIGNMENT, A>>;
