mod buffer_pool;

#[cfg(feature = "rkyv")]
mod serializer_pool;

#[cfg(all(feature = "rkyv", feature = "bytecheck"))]
mod validator_pool;

pub use aligned_buffer;
pub use buffer_pool::{
	AlignedBufferPool, BufferPoolAllocator, BufferRetentionPolicy, ConstMaxSizeRetentionPolicy,
	RetainAllRetentionPolicy, SharedPooledAlignedBuffer, UniquePooledAlignedBuffer,
	WeakAlignedBufferPool,
};

#[cfg(feature = "rkyv")]
pub use serializer_pool::{
	Serializer, SerializerAlignedBuffer, SerializerPool, SerializerPoolAllocator, SerializerWeakRef,
};

#[cfg(all(feature = "rkyv", feature = "bytecheck"))]
pub use validator_pool::{PooledValidator, ValidatorPool};
