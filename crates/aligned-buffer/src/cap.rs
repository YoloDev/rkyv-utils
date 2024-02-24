#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Cap(pub(crate) crate::raw::TaggedCap);
