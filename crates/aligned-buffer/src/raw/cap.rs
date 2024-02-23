use std::fmt;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TaggedCap {
	value: usize,
}

impl TaggedCap {
	const TAG: usize = !(usize::MAX >> 1);
	const TAG_MASK: usize = Self::TAG;
	const VALUE_MASK: usize = !Self::TAG_MASK;

	pub const MAX_VALUE: usize = Self::VALUE_MASK;

	pub const fn zero() -> Self {
		Self { value: 0 }
	}

	#[inline]
	pub const fn new(value: usize, tag: bool) -> Self {
		TaggedCap::zero().with_value(value).with_allocated(tag)
	}

	#[inline(always)]
	pub const fn value(self) -> usize {
		self.value & Self::VALUE_MASK
	}

	#[inline(always)]
	pub const fn is_allocated(self) -> bool {
		self.value & Self::TAG_MASK != 0
	}

	#[inline(always)]
	pub const fn with_value(self, value: usize) -> Self {
		debug_assert!(value <= Self::VALUE_MASK, "Value overflow");
		let value = (self.value & Self::TAG_MASK) | (value & Self::VALUE_MASK);
		Self { value }
	}

	#[inline(always)]
	pub const fn with_allocated(self, allocated: bool) -> Self {
		let value_part = self.value & Self::VALUE_MASK;
		let tag_part = ((Self::TAG - 1) + allocated as usize) & Self::TAG_MASK;
		let value = value_part | tag_part;
		Self { value }
	}

	#[inline(always)]
	pub const fn into_inner(self) -> usize {
		self.value
	}

	#[inline(always)]
	pub const unsafe fn from_inner(value: usize) -> Self {
		Self { value }
	}
}

impl fmt::Debug for TaggedCap {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TaggedCap")
			.field("value", &self.value())
			.field("allocated", &self.is_allocated())
			.finish()
	}
}

static_assertions::const_assert!(TaggedCap::zero().value() == 0);
static_assertions::const_assert!(!TaggedCap::zero().is_allocated());
