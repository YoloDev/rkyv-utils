use crossbeam_queue::ArrayQueue;

/// A pool allocator. Responsible for creating new objects of type T,
/// as well as recycling them.
pub trait PoolAllocator<T> {
	/// Creates a new object of type T.
	fn allocate(&self) -> T;

	/// Resets the state of an object to its initial state if necessary.
	///
	/// By default, this method do nothing. Override this method to provide
	/// custom reset logic.
	#[inline(always)]
	fn reset(&self, _obj: &mut T) {}

	/// validates that an object is in a good state to be stored back in the
	/// object pool.
	///
	/// By default, this method always returns true. Override this method to
	/// provide custom validation logic.
	#[inline(always)]
	fn is_valid(&self, _obj: &T) -> bool {
		true
	}
}

/// A struct representing an object pool.
///
/// This struct uses an allocator to create and manage objects, and stores them
/// in an ArrayQueue.
#[derive(Debug)]
pub struct Pool<P: PoolAllocator<T>, T> {
	allocator: P,
	storage: ArrayQueue<T>,
}

// If T is Send it is safe to move object pool between threads
unsafe impl<P: PoolAllocator<T>, T: Send> Send for Pool<P, T> {}

impl<P: PoolAllocator<T>, T> Pool<P, T> {
	/// Creates a new object pool with the given allocator and capacity.
	pub fn new(allocator: P, capacity: usize) -> Self {
		Self {
			allocator,
			storage: ArrayQueue::new(capacity),
		}
	}

	/// Gets an object from the pool. If the pool is empty, a new object is
	/// allocated and returned.
	pub fn get(&self) -> T {
		self
			.storage
			.pop()
			.unwrap_or_else(|| self.allocator.allocate())
	}

	/// Recycles an object, putting it back into the pool.
	pub fn recycle(&self, mut obj: T) {
		if self.allocator.is_valid(&obj) {
			self.allocator.reset(&mut obj);
			let _ = self.storage.push(obj);
		}
	}
}
