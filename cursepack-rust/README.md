# cursepack (Rust) — v0.3

A Rust library for reading and writing **Cursepack v0.3** cursor-theme files. It keeps the public API from the supplied v0.2 Rust library while changing the on-disk metadata layout to the v0.3 format.

## Compatibility with the supplied library

These public entry points and core types are retained:

- `encode_theme(input_dir, output, ThemeInfo) -> Result<Metadata>`
- `decode_theme(input, output_dir) -> Result<Vec<PathBuf>>`
- `inspect(input) -> Result<Metadata>`
- `ThemeInfo`, `Metadata`, `AuthorMetadata`, `CursorMetadata`, `SeparatorMetadata`
- `CursepackError` and `Result<T>`

`Metadata` remains a flattened, convenient Rust view so existing code can continue reading fields such as `metadata.name`, `metadata.cursor_count`, and `metadata.version`. The `format`, `container`, `encoding`, and `notes` fields are compatibility conveniences synthesized by the library; they are **not written into the v0.3 file**. On disk, metadata is split between `CPKD` and `CPKC` exactly as specified by Cursepack v0.3.

This is a format-version change, not a v0.2 reader. The library expects exactly one `CPKD` and one `CPKC` chunk. It does not read old v0.2 `CPKM` files.

## Add to your project

```toml
[dependencies]
cursepack = { path = "../Cursepack-rust-v0.3" }
```

Or publish the crate and use its registry version.

## Example

```rust
use cursepack::{decode_theme, encode_theme, inspect, ThemeInfo};
use std::path::Path;

fn main() -> cursepack::Result<()> {
    let theme = ThemeInfo {
        name: "My cursors".into(),
        author: "Example author".into(),
        author_url: String::new(),
        theme_url: String::new(),
        description: "A cursor theme".into(),
        license: "CC0-1.0".into(),
    };

    let metadata = encode_theme("./cursor-source", "./my-theme.cursepack", theme)?;
    println!("Encoded {} cursor roles", metadata.cursor_count);

    let inspected = inspect("./my-theme.cursepack")?;
    println!("Theme: {}", inspected.name);

    let files = decode_theme("./my-theme.cursepack", "./decoded")?;
    println!("Decoded {} cursor files", files.len());
    Ok(())
}
```

`cursor-source` must contain an `install.inf` file mapping Windows cursor roles to `.cur` or `.ani` files, as in the original library.

## Dependencies and implementation

- Animated WebP is encoded and decoded in-process through `webp-animation` / libwebp.
- CUR, ANI, RIFF framing, metadata, hotspot marker and separator handling are implemented in this crate.
- No Python or ImageMagick process is required.

## Decode output

The decoder writes the reconstructed `.cur` / `.ani` files plus two JSON files:

- `descriptor.json` — the `CPKD` descriptor object.
- `cursors.json` — the `CPKC` cursor/frame object.

## Important limitations inherited from the original implementation

- CUR decoding supports PNG-backed images and uncompressed 24/32-bit DIB images.
- ANI decoding reconstructs the frame sequence and timing, but does not promise byte-identical preservation of every unusual ANI sequence/header feature.
- The Rust ANI writer emits a straightforward frame-per-step ANI.
- v0.3 files are not backward-compatible with v0.2.

## Build and tests

```sh
cargo test
```

The test suite includes a real Python-generated v0.3 Wii cursor fixture, metadata chunk tests, and a Rust encode/decode round-trip test.
