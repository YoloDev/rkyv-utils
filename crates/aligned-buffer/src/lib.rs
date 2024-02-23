mod raw;
mod shared;
mod unique;

pub mod alloc;

#[cfg(feature = "bytes")]
mod bytes;

#[cfg(feature = "rkyv")]
pub mod rkyv;

pub use shared::SharedAlignedBuffer;
pub use unique::{TryReserveError, UniqueAlignedBuffer};
