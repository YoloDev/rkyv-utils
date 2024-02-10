use std::{alloc, cmp};

#[derive(Clone, Copy)]
pub struct LayoutHelper {
	layout: alloc::Layout,
}

impl LayoutHelper {
	pub const fn new<T: Sized>() -> Self {
		Self::from_layout(alloc::Layout::new::<T>())
	}

	pub const fn size(&self) -> usize {
		self.layout.size()
	}

	pub const fn align(&self) -> usize {
		self.layout.align()
	}

	pub const fn from_layout(layout: alloc::Layout) -> Self {
		Self { layout }
	}

	pub const fn into_layout(self) -> alloc::Layout {
		self.layout
	}

	pub const fn from_size_alignment(size: usize, alignment: usize) -> Self {
		match alloc::Layout::from_size_align(size, alignment) {
			Ok(layout) => Self::from_layout(layout),
			Err(_) => panic!("Invalid layout"),
		}
	}

	pub const fn padding_needed_for(&self, align: usize) -> usize {
		let len = self.size();

		// Rounded up value is:
		//   len_rounded_up = (len + align - 1) & !(align - 1);
		// and then we return the padding difference: `len_rounded_up - len`.
		//
		// We use modular arithmetic throughout:
		//
		// 1. align is guaranteed to be > 0, so align - 1 is always
		//    valid.
		//
		// 2. `len + align - 1` can overflow by at most `align - 1`,
		//    so the &-mask with `!(align - 1)` will ensure that in the
		//    case of overflow, `len_rounded_up` will itself be 0.
		//    Thus the returned padding, when added to `len`, yields 0,
		//    which trivially satisfies the alignment `align`.
		//
		// (Of course, attempts to allocate blocks of memory whose
		// size and padding overflow in the above manner should cause
		// the allocator to yield an error anyway.)

		let len_rounded_up = len.wrapping_add(align).wrapping_sub(1) & !align.wrapping_sub(1);
		len_rounded_up.wrapping_sub(len)
	}

	pub const fn extend(self, next: Self) -> (Self, usize) {
		// cmp::max(self.align(), next.align());
		let new_align = match (self.align(), next.align()) {
			(l, r) if l < r => r,
			(l, _) => l,
		};
		let pad = self.padding_needed_for(next.align());

		let Some(offset) = self.size().checked_add(pad) else {
			panic!("Invalid layout: offset overflow");
		};
		let Some(new_size) = offset.checked_add(next.size()) else {
			panic!("Invalid layout: size overflow");
		};

		// The safe constructor is called here to enforce the isize size limit.
		let layout = Self::from_size_alignment(new_size, new_align);
		(layout, offset)
	}
}
