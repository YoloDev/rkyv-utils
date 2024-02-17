check: lint test

lint:
	cargo clippy --all -- -D warnings

test:
	cargo test --workspace --all-features

miri-test:
	MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --workspace --all-features
