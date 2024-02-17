mod raw;
mod shared;
mod unique;

#[cfg(feature = "bytes")]
mod bytes;

#[cfg(feature = "rkyv")]
pub mod rkyv;

pub use shared::SharedAlignedBuffer;
pub use unique::{TryReserveError, UniqueAlignedBuffer};
