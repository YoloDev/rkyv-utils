miri-test:
	MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --workspace --all-features
