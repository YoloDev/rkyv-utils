use crate::{raw::RawAlignedBuffer, UniqueAlignedBuffer};
use bytes::{buf::UninitSlice, BufMut};

unsafe impl<const ALIGNMENT: usize> BufMut for UniqueAlignedBuffer<ALIGNMENT> {
	#[inline]
	fn remaining_mut(&self) -> usize {
		RawAlignedBuffer::<ALIGNMENT>::MAX_CAPACITY - self.len()
	}

	#[inline]
	unsafe fn advance_mut(&mut self, cnt: usize) {
		let new_len = self.len() + cnt;
		assert!(
			new_len <= self.capacity(),
			"new_len = {}; capacity = {}",
			new_len,
			self.capacity()
		);

		self.set_len(new_len);
	}

	#[inline]
	fn chunk_mut(&mut self) -> &mut UninitSlice {
		if self.capacity() == self.len() {
			self.reserve(64);
		}

		let start = self.len();
		let len = self.capacity() - start;

		unsafe {
			let start_remaining = self.as_mut_ptr().add(len);
			UninitSlice::from_raw_parts_mut(start_remaining, len)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn write_using_buf() {
		fn do_write(buf: &mut impl BufMut) {
			for i in 0..200 {
				buf.put_u64_le(i);
			}
		}

		let mut buf = UniqueAlignedBuffer::<16>::new();
		assert_eq!(buf.capacity(), 0);
		assert_eq!(buf.len(), 0);

		do_write(&mut buf);
		assert_eq!(buf.len(), 200 * 8);
		assert!(buf.capacity() >= 200 * 8);
	}

	#[test]
	fn write_using_buf_pathological() {
		fn do_write(buf: &mut impl BufMut) {
			for i in 0u8..200 {
				// Write 3 bytes such that we are guarenteed to not hit allocation boundaries
				// every now and again.
				buf.put_slice(&[i, i + 1, i + 2]);
			}
		}

		let mut buf = UniqueAlignedBuffer::<16>::new();
		assert_eq!(buf.capacity(), 0);
		assert_eq!(buf.len(), 0);

		do_write(&mut buf);
		assert_eq!(buf.len(), 200 * 3);
		assert!(buf.capacity() >= 200 * 3);
	}
}
