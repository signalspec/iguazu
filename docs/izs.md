# The Iguazu `.izs` file format

## Design Goals

 - Container for multiple streams of timeseries data including analog and digital waveforms and events with extensible metadata following the [Iguazu data model](./data_model.md)
 - Multiple compression options optimal for different data types
 - Efficient random access via HTTP range requests
 - Sufficient performance to read and write in real time on low-end / embedded hardware
 - Write in a single pass for generation during streaming upload

## Specification

An `.izs` file is read starting with the footer at the end of the file. Immediately prior to the footer, the metadata block contains the schema along with pointers to per-stream indexes. These indexes hold the pointers to each data block of the stream.

<img src=izs.svg width=1000 alt="File layout diagram" />

### Header

The file begins with the 8 byte header

```
0x00, 0x21, 0x4a, 0xd9, 0xff, 0x90, 0xba, 0xed
```

This header is not needed to read the file, but exists to allow identification of the file format.

### Data blocks

The bulk of the file consists of stream data in compressed blocks. Each block represents a fixed number of elements from one stream, but is variable-length as stored due to compression. Only the final block of each stream may be shorter than the block size. The block offsets and sizes within the file are located via references in the block index.

Each block is individually compressed with a method specified in the metadata.

#### Compression methods

  - `none`: array of little-endian elements is stored directly without compression.
  - `zstd`: array of little-endian elements is compressed with [Zstandard].
  - *Future:* Investigate [pcodec]

[Zstandard]: https://www.rfc-editor.org/rfc/rfc8478
[pcodec]: https://github.com/pcodec/pcodec

### Block index

Each stream has one block index containing the positions and compressed sizes of all of that stream's data blocks.

The block index must come after all data blocks of that stream. Normally all block indexes are at the end of the file, just before the metadata, but it is permitted to interleave block indexes with data blocks of other streams, as may occur when appending data to an existing file.

The format and compression of the index block is specified in the `i_compress` field in the schema:

* `none`: uncompressed array of 64-bit little endian offsets, followed by an array of 32-bit little endian sizes.
* *Future:* pcodec

### Schema

The schema at the end of the file is [Zstandard]-compressed JSON.

The top-level object has an `"entity"` property containing the JSON encoding of the Iguazu schema format. Each data stream entity within the entity tree has a `"data"` property, which is a stream descriptor referring to the data of the stream. The JSON object has properties:

  - `element`: `"u8"`, `"u16"`, `"u32"`, `"u64"` representing the bit width of each element.
  - `i_offset`: Integer offset of this stream's block index in bytes from the start of the file.
  - `i_size`: Integer size in bytes of the block index after compression.
  - `i_compress`: Compression format for the block index. See the [Block index](#block-index) section for definitions.
  - `block`: Count of elements per block. This should be a power of 2 and greater than or equal to 4096.
  - `compress`: Compression method used for data blocks. See the [Data blocks](#data-blocks) section for definitions.
  - `end`: Total number of elements in the stream.

An example schema for a file containing two logic analyzer channels:

```json
{
  "entity": {
    "type": "bit_struct",
    "children": {
      "sda": {
        "type": "bits",
        "bits": 1,
        "display:color": "neutral"
      },
      "scl": {
        "type": "bits",
        "pos": 1,
        "bits": 1,
        "display:color": "brown"
      }
    },
    "time:rate": 8000000.0,
    "data": {
      "element": "u8",
      "block": 1048576,
      "compress": "zstd",
      "i_offset": 16612,
      "i_size": 156,
      "i_compress": "none",
      "end": 13348017
    },
    "summaries": {
      "bit_and_or": {
        "base_level": 2,
        "levels": [
          {
            "element": "u8",
            "block": 1048576,
            "compress": "zstd",
            "i_offset": 16336,
            "i_size": 84,
            "i_compress": "none",
            "end": 6674008
          },
          {
            "element": "u8",
            "block": 1048576,
            "compress": "zstd",
            "i_offset": 16420,
            "i_size": 48,
            "i_compress": "none",
            "end": 3337004
          },
          // more levels omitted
        ]
      }
    }
  }
}
```
    
### Footer

The file ends with a 16 byte footer.

  - Bytes 0-4: little-endian integer: compressed length of the schema immediately preceding the footer.
  - Bytes 4-8: reserved. Must be `0`.
  - Bytes 8-16: bytes `0x01, 0x21, 0x4a, 0xd9, 0x01, 0x90, 0xba, 0xed`

### Checkpoints

Because the footer, schema, and indexes are placed at the end of the file, a truncated file is unusable. To minimize data loss after unexpected interruption during long-term data collection, a to-be-defined extension will allow periodically checkpointing the unwritten partial data blocks, block indexes, and schema in a separate file that can be merged with the main data file when recovery is necessary.
