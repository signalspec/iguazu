# Import and Export Formats

Importers open an input file and produce an Iguazu entity tree and streams. Some importers parse and copy the data to streams created in a default storage backend, while others create streams with a storage backend that loads data lazily from the source file. Importers load a schema provided in the file, or infer the schema from the file contents. If a different schema is provided on the command line, some importers can parse the file according to that schema rather than the inferred one.

An importer and its options can be specified on the command line with the `-f format:option1=value1:option2=value2` syntax.

In the GUI, the options are prompted after opening a file.

## Raw (`raw`)

Array of raw samples in a file.

A schema is generated from the `dtype` and `sample_rate` options. If a schema is provided, those options are ignored. The schema must be a single data entity (containing arbitrary fields), or a tuple wrapping a single data entity.

Files with the following extensions are detected by this importer, with corresponding default option values:
`.bin`, `.f32`, `.cf32`, `.cfile`, `.u8`, `.u16`, `.u32`, `.u64`, `.s8`, `.s16`, `.s32`, `.s64`, `.cu8`, `.cu16`, `.cu32`, `.cu64`, `.cs8`, `.cs16`, `.cs32`, `.cs64`, `.logic8`

The data is loaded from the file lazily as needed.

### Import options

  - **`bits`**: Element size in bits (8, 16, 32, 64)
  - **`dtype`**: Data type ("b" | "binary", "l" | "logic", "u" | "unsigned", "s" | "signed", "f" | "float", "cu" | "complex_unsigned", "cs" | "complex_signed", "cf" | "complex_float")
  - **`sample_rate`**: Sample rate.
  - **`offset`**: Byte offset in the file where the data starts. Default 0.
  - **`count`**: Number of elements to read from the file. Empty means to read until the end of the file.
  - **`block_size`**: Number of elements to read in each block.

## CSV / TSV (`csv`, `tsv`)

The ubiquitous tabular data format.

Data is parsed into streams in the default storage backend.

Iguazu can infer a schema for columns containing:
  - ISO 8601 absolute timestamps
  - Relative timestamps (detected in the first column or a column named `t`, `time`, or `timestamp`, containing monotonically increasing values)
  - Numbers as float32
  - Enums (detected when all values <= 15 characters, fewer than 32 distinct values)
  - Strings (fallback if none of the heuristics match)

If a schema is provided, the columns will be parsed according to the schema for supported types.

It's recommended to use `iguazu schema file.csv > schema.json` to infer a schema, review and edit it, then pass `-s schema.json` to further commands to ensure you get a consistent schema.

### Import options

  - **`delimiter`**: Delimiter byte, defaults to `,` for CSV and `\t` for TSV.
  - **`terminator`**: Record terminator byte. If empty, either `\n` or `\r\n` is accepted.
  - **`quote`**: Quote byte. Empty or `none` disable quoting.
  - **`escape`**: Escape byte before quotes. Empty or `none` disable escaping.
  - **`double_quote`**: Whether to interpret doubled quote characters as an escaped quote.
  - **`comment`**: Comment byte. If specified, lines beginning with this byte will be skipped.
  - **`skip`**: Number of lines to skip before reading headers.
  - **`columns`**: Comma-separated list of column names in place of the header line. If empty, the first line of the file (after skip) is used as a header.

## Sigrok srzip v2 (`sigrok`)

The zip-based file format from Sigrok and PulseView.

Detected from a `.sr` file extension.

The schema is generated from the digital and analog channels in the file metadata and cannot currently be overridden.

## izs (`izs`)

The native [Iguazu Signal](./izs.md) format.

The schema comes from the file and currently cannot be overridden. The data is loaded and decompressed lazily as needed.

## Virtual (`virtual`)

An `.iguazu.json` JSON file containing a schema along with references to data in a flat file or stored inline.

Inspired by [GDAL's VRT][VRT].

[VRT]: https://gdal.org/en/stable/drivers/raster/vrt.html
