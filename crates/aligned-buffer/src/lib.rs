mod raw;
mod shared;
mod unique;

pub use shared::SharedAlignedBuffer;
pub use unique::{TryReserveError, UniqueAlignedBuffer};
