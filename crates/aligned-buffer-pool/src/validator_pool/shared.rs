use fxhash::FxHashMap;
use rkyv::{
	rancor::{fail, Source},
	validation::{shared::ValidationState, SharedContext},
};
use std::{any::TypeId, collections::hash_map::Entry, error::Error, fmt};

#[derive(Debug)]
struct TypeMismatch {
	previous: TypeId,
	current: TypeId,
}

impl fmt::Display for TypeMismatch {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"the same memory region has been claimed as two different types: \
             {:?} and {:?}",
			self.previous, self.current,
		)
	}
}

impl Error for TypeMismatch {}

#[derive(Debug)]
struct NotStarted;

impl fmt::Display for NotStarted {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "shared pointer was not started validation")
	}
}

impl Error for NotStarted {}

#[derive(Debug)]
struct AlreadyFinished;

impl fmt::Display for AlreadyFinished {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "shared pointer was already finished validation")
	}
}

impl Error for AlreadyFinished {}

#[derive(Debug, Default)]
pub(super) struct SharedValidator {
	shared: FxHashMap<usize, (TypeId, bool)>,
}

impl SharedValidator {
	pub fn clear(&mut self) {
		self.shared.clear();
	}
}

impl<E: Source> SharedContext<E> for SharedValidator {
	fn start_shared(&mut self, address: usize, type_id: TypeId) -> Result<ValidationState, E> {
		match self.shared.entry(address) {
			Entry::Vacant(vacant) => {
				vacant.insert((type_id, false));
				Ok(ValidationState::Started)
			}
			Entry::Occupied(occupied) => {
				let (previous_type_id, finished) = occupied.get();
				if previous_type_id != &type_id {
					fail!(TypeMismatch {
						previous: *previous_type_id,
						current: type_id,
					})
				} else if !finished {
					Ok(ValidationState::Pending)
				} else {
					Ok(ValidationState::Finished)
				}
			}
		}
	}

	fn finish_shared(&mut self, address: usize, type_id: TypeId) -> Result<(), E> {
		match self.shared.entry(address) {
			Entry::Vacant(_) => fail!(NotStarted),
			Entry::Occupied(mut occupied) => {
				let (previous_type_id, finished) = occupied.get_mut();
				if previous_type_id != &type_id {
					fail!(TypeMismatch {
						previous: *previous_type_id,
						current: type_id,
					});
				} else if *finished {
					fail!(AlreadyFinished);
				} else {
					*finished = true;
					Ok(())
				}
			}
		}
	}
}
