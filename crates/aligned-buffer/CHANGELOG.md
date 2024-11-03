# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0](https://github.com/YoloDev/rstml-component/compare/0.1.0..0.2.0) - 2024-11-03

### ✨ Features

- BufMut with custom allocator - ([4a86774](https://github.com/YoloDev/rstml-component/commit/4a86774095017a6a88a761a89b19b35057219f64))

## [0.1.0](https://github.com/YoloDev/rstml-component/compare/0.0.9..0.1.0) - 2024-11-02

### ✨ Features

- Better cargo features - ([6634964](https://github.com/YoloDev/rstml-component/commit/6634964fa06e64af8011ce4c72c8d3e929b7a8a0))
- Rkyv 0.8 - ([026b6da](https://github.com/YoloDev/rstml-component/commit/026b6da2b34a666c8ee9e07d360af5632a37c3fa))

## [0.0.9](https://github.com/YoloDev/rstml-component/compare/0.0.8..0.0.9) - 2024-06-24

### 🐛 Bug Fixes

- Impl Default on SharedAlignedBuffer - ([61d0601](https://github.com/YoloDev/rstml-component/commit/61d060105ac244d91a7ba9319da01bc286216dc3))

### 📚 Documentation

- Fix doc links - ([8772602](https://github.com/YoloDev/rstml-component/commit/87726028ae1c23ff8bfbbea3a3a4ec540fe14eba))

## [0.0.8](https://github.com/YoloDev/rstml-component/compare/0.0.7..0.0.8) - 2024-03-24

### ✨ Features

- Provide default alignment - ([cd0b9bf](https://github.com/YoloDev/rstml-component/commit/cd0b9bfe0ef2778759e273633747984d5ec36399))

## [0.0.7](https://github.com/YoloDev/rstml-component/compare/0.0.6..0.0.7) - 2024-03-24

### ✨ Features

- Update to new rkyv alpha - ([3f35dc4](https://github.com/YoloDev/rstml-component/commit/3f35dc4bf062dbd33e32666495ae0fa167ba76ce))

## [0.0.6](https://github.com/YoloDev/rstml-component/compare/0.0.5..0.0.6) - 2024-02-24

### ✨ Features

- Buffers return themselves to pools - ([8dcdcce](https://github.com/YoloDev/rstml-component/commit/8dcdcce7c42fa2c1153b29c5b039a412a21a1273))
- Better alloc/raw apis - ([cde637d](https://github.com/YoloDev/rstml-component/commit/cde637d3e4caf95a45302b44439dc06deb388f4b))

## [0.0.5](https://github.com/YoloDev/rstml-component/compare/0.0.4..0.0.5) - 2024-02-23

### ✨ Features

- To/from raw parts - ([1a1c5fa](https://github.com/YoloDev/rstml-component/commit/1a1c5fad8b09b5692db65d0982e4f8ae7501e81a))
- Custom allocator - ([1730d01](https://github.com/YoloDev/rstml-component/commit/1730d0100a6502a5eb115ad7c352c6fefff8871c))

## [0.0.4](https://github.com/YoloDev/rstml-component/compare/0.0.3..0.0.4) - 2024-02-23

### ✨ Features

- Aligned-buffer-pool - ([6a86765](https://github.com/YoloDev/rstml-component/commit/6a86765cc665c1c9749c895d394ebd5bb9627fb8))
- Add rkyv traits - ([34e69a2](https://github.com/YoloDev/rstml-component/commit/34e69a2d92e6248ea2bcb6a939d1978f470d1f12))
- Allow shared buffers to have larger capacity than length - ([bcd38f1](https://github.com/YoloDev/rstml-component/commit/bcd38f1441baaf66d6b70c3e55c57664a85e787d))
- Try_unique - ([87e26d8](https://github.com/YoloDev/rstml-component/commit/87e26d88431454db41b5f63c797e05dfb339a6aa))
- Shared buffer - ([5b03e9b](https://github.com/YoloDev/rstml-component/commit/5b03e9bd4cd99b0b03e5cc0142e6ee4f91d29a5c))
- Unique buffer - ([0bfa276](https://github.com/YoloDev/rstml-component/commit/0bfa276d9d0d382553a7b9e81c8c37c251c9baee))

### 🐛 Bug Fixes

- Bad cargo.toml, and some unused code - ([56ecf4f](https://github.com/YoloDev/rstml-component/commit/56ecf4fdb27c9d9be3c4734bd0b4c6f8e3ea5c97))
- Wrong pointer offset - ([d30fb47](https://github.com/YoloDev/rstml-component/commit/d30fb4713ef7faae6a5b41528ad930bda36bc60a))

### 👷 CI

- Add some package metadata - ([5b9451a](https://github.com/YoloDev/rstml-component/commit/5b9451a3ed145ea3537c8589c14933f8c6d0397a))

### 🔨 Chore

- Ungit rkyv - ([5f9d0ee](https://github.com/YoloDev/rstml-component/commit/5f9d0eefab191d2be3f16106d9987bb21a3f3f89))
- Add some more testing - ([5286b07](https://github.com/YoloDev/rstml-component/commit/5286b07c7257dd4842c892e0ad59647c8de8c6bb))
- Improve some `BufMut` methods - ([c5024e6](https://github.com/YoloDev/rstml-component/commit/c5024e6fcf3d171988b099dd1730bbe787c02368))
- Add `bytes::BufMut` support - ([7e7a2a4](https://github.com/YoloDev/rstml-component/commit/7e7a2a46969ac9163bfd0d69964ed21f73ce85c8))
- Fix clippy warning - ([29fd0d8](https://github.com/YoloDev/rstml-component/commit/29fd0d8f63ac78e398e331fd034e5164286dfc4c))
- Initial commit - ([6205383](https://github.com/YoloDev/rstml-component/commit/62053833c51f864c7ed8243bc78b179cf401bd95))

## [0.0.3](https://github.com/YoloDev/rstml-component/compare/0.0.2..0.0.3) - 2024-02-11

### 🐛 Bug Fixes

- Wrong pointer offset - ([d30fb47](https://github.com/YoloDev/rstml-component/commit/d30fb4713ef7faae6a5b41528ad930bda36bc60a))

### 🔨 Chore

- Add some more testing - ([5286b07](https://github.com/YoloDev/rstml-component/commit/5286b07c7257dd4842c892e0ad59647c8de8c6bb))
- Improve some `BufMut` methods - ([c5024e6](https://github.com/YoloDev/rstml-component/commit/c5024e6fcf3d171988b099dd1730bbe787c02368))
- Add `bytes::BufMut` support - ([7e7a2a4](https://github.com/YoloDev/rstml-component/commit/7e7a2a46969ac9163bfd0d69964ed21f73ce85c8))

## [0.0.2](https://github.com/YoloDev/rstml-component/compare/0.0.1..0.0.2) - 2024-02-11

### ✨ Features

- Allow shared buffers to have larger capacity than length - ([bcd38f1](https://github.com/YoloDev/rstml-component/commit/bcd38f1441baaf66d6b70c3e55c57664a85e787d))
- Try_unique - ([87e26d8](https://github.com/YoloDev/rstml-component/commit/87e26d88431454db41b5f63c797e05dfb339a6aa))

### 🔨 Chore

- Fix clippy warning - ([29fd0d8](https://github.com/YoloDev/rstml-component/commit/29fd0d8f63ac78e398e331fd034e5164286dfc4c))

## [0.0.1] - 2024-02-10

### ✨ Features

- Shared buffer - ([5b03e9b](https://github.com/YoloDev/rstml-component/commit/5b03e9bd4cd99b0b03e5cc0142e6ee4f91d29a5c))

