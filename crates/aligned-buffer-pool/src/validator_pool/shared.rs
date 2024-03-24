use fxhash::FxHashMap;
use rkyv::{
	rancor::{fail, Error},
	validation::SharedContext,
};
use std::{any::TypeId, collections::hash_map::Entry, fmt};

/// Errors that can occur when checking shared memory.
#[derive(Debug)]
pub enum SharedError {
	/// Multiple pointers exist to the same location with different types
	TypeMismatch {
		/// A previous type that the location was checked as
		previous: TypeId,
		/// The current type that the location is checked as
		current: TypeId,
	},
}

impl fmt::Display for SharedError {
	#[inline]
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			SharedError::TypeMismatch { previous, current } => write!(
				f,
				"the same memory region has been claimed as two different types ({:?} and {:?})",
				previous, current
			),
		}
	}
}

impl std::error::Error for SharedError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			SharedError::TypeMismatch { .. } => None,
		}
	}
}

#[derive(Debug, Default)]
pub(super) struct SharedValidator {
	shared: FxHashMap<usize, TypeId>,
}

impl SharedValidator {
	pub fn clear(&mut self) {
		self.shared.clear();
	}
}

impl<E: Error> SharedContext<E> for SharedValidator {
	#[inline]
	fn register_shared_ptr(&mut self, address: usize, type_id: TypeId) -> Result<bool, E> {
		match self.shared.entry(address) {
			Entry::Occupied(previous_type_entry) => {
				let previous_type_id = previous_type_entry.get();
				if previous_type_id != &type_id {
					fail!(SharedError::TypeMismatch {
						previous: *previous_type_id,
						current: type_id,
					})
				} else {
					Ok(false)
				}
			}

			Entry::Vacant(ent) => {
				ent.insert(type_id);
				Ok(true)
			}
		}
	}
}
