mod raw;
mod shared;
mod unique;

pub mod alloc;
pub mod cap;

#[cfg(feature = "bytes")]
mod bytes;

#[cfg(feature = "rkyv")]
pub mod rkyv;

/// The default alignment for buffers.
pub const DEFAULT_BUFFER_ALIGNMENT: usize = 64;

pub use shared::SharedAlignedBuffer;
pub use unique::{TryReserveError, UniqueAlignedBuffer};
