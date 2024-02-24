#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Cap(pub(crate) crate::raw::TaggedCap);

impl Cap {
	/// Gets the size of the capacity.
	pub const fn size(&self) -> usize {
		self.0.value()
	}
}
