use crate::raw::RawAlignedBuffer;

pub struct UniqueAlignedBuffer<const ALIGNMENT: usize> {
	buffer: RawAlignedBuffer<ALIGNMENT>,
	len: usize,
}
