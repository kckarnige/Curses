# Cursepack v0.3 format specification

## Container

A `.cursepack` v0.3 is an animated WebP RIFF container. The image data is always **lossless animated WebP**; the container and encoding are fixed by the format and are not configurable metadata fields.

Two custom RIFF chunks follow the WebP image chunks:

- `CPKD`: UTF-8 JSON theme descriptor.
- `CPKC`: UTF-8 JSON cursor/frame data.

Each chunk uses standard RIFF framing: four-byte identifier, little-endian 32-bit payload length, UTF-8 JSON payload and a zero pad byte for odd payload lengths. The RIFF size includes both chunks.

## Descriptor (`CPKD`)

| Field | Meaning |
|---|---|
| `format_version` | Cursepack metadata/layout version (`3`) |
| `name` | Theme name |
| `author.name` | Author name |
| `author.url` | Author homepage/profile |
| `theme_url` | Original theme/download URL |
| `description` | Theme description |
| `license` | License identifier or text |

No `container`, `encoding`, or repeated format-name field is needed.

## Cursor data (`CPKC`)

Top-level fields:

- `canvas`: common WebP canvas `[width, height]`.
- `cursor_count`: number of cursor roles.
- `frame_count_total`: total WebP frame count, including separators.
- `cursors`: ordered cursor records.
- `separator`: signature and separator frame indices.

Each cursor record contains:

- `role`: Windows cursor role (e.g. `Arrow`, `Wait`, `SizeWE`).
- `source_filename` and `source_format` (`cur` or `ani`).
- `start_frame` and `frame_count`.
- `frame_sizes`: original dimensions per cursor frame.
- `frame_hotspots`: hotspot `[x, y]` per cursor frame.
- `frame_delays_ms`: source delay per frame.
- `marker_rgba`: exact marker color in the first frame.
- `marker_original_rgba`: source pixel replaced by the marker.
- `ani_info`: optional source ANI header details.

## Image and frame sequence

All cursor frames are full-canvas RGBA images padded transparently at the top-left. Original dimensions are stored in `CPKC` and used to crop during decoding. For each cursor, its one or more image frames are followed by exactly one separator frame. The first cursor frame contains one unique opaque hotspot marker pixel.

The separator uses the `CPKSEP02` signature in the top-left 32×32 pixels. Each signature bit occupies a 4×4 block: set bits are `(255, 0, 255, 255)`, clear bits `(0, 255, 255, 255)`, with `(19, 23, 29, 255)` as the remaining background.

The marker RGB starts at the average opaque source color, then varies green to find a color unused by opaque source pixels. If necessary, it searches the RGB space. The marker replaces the original pixel at the hotspot; both marker and overwritten RGBA values are recorded in `CPKC`.

## Decoding

The decoder requires exactly one `CPKD` and one `CPKC` chunk, validates `format_version`, frame count, separator frames and marker coordinates, then reconstructs `.cur` and `.ani` files. It also writes the two metadata documents separately as `descriptor.json` and `cursors.json`.

## Compatibility

v0.3 replaces v0.2's single `CPKM` JSON chunk. A v0.2 reader will not understand v0.3, and vice versa. The underlying image remains animated WebP; third-party tools may ignore unknown chunks, but re-saving may strip metadata or alter the frame sequence.
