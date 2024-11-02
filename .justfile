check: lint test

lint:
	# cargo clippy --all -- -D warnings
	cargo featurex clippy

test:
	# cargo test --workspace --all-features
	cargo featurex test

miri-test:
	MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --workspace --all-features
