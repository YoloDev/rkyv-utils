mod raw;
mod shared;
mod unique;

#[cfg(feature = "bytes")]
mod bytes;

#[cfg(feature = "rkyv")]
mod rkyv;

pub use shared::SharedAlignedBuffer;
pub use unique::{TryReserveError, UniqueAlignedBuffer};
