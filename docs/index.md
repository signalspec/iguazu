# Iguazu

Tools for viewing, storing, and sharing mixed-signal time series data

[Source code on GitHub](https://github.com/signalspec/iguazu) | [Online demo](https://app.iguazu.signalspec.org)

## Key features

- Import from CSV, Sigrok srzip, and raw binary array files. *Future: JSON, WAV, SigMF, VCD, etc.*

- A native [`.izs` (Iguazu Signal) file format](./izs.md) supporting compression, metadata, random access, and incremental loading from static web hosting.

- [`Stream` abstraction layer](./data_model.md#streams) for linear or random access to blocks of raw samples in memory, on disk, or from a remote server

- [Schema layer](./data_model.md#entities) for interpreting raw samples as bit structs, fixed or floating point numbers, timestamps, text, enums, variable or fixed-length arrays/packets, records, and hierarchical groups.

- Flexible metadata [attributes](./attributes.md)

- Zoomable timeline viewer with analog signal plots, digital logic traces and event spans. *Future: spectrograms*

- Table view

- Rust library, [CLI](./cli.md), egui-based viewer app for Linux, macOS, Windows, and web

## Screenshot

<img src="https://kevinmehall.net/2026/iguazu_screenshot_001.png" alt="Screenshot" width="1036" />
