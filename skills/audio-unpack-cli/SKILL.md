---
name: audio-unpack-cli
description: "Build, run, test, and modify the local audio-unpack-cli Rust project at E:\\Projects\\github\\ncmdumpGUI-1.2. Use when Codex needs to work on the command-line audio container decoder itself, especially for: (1) building or releasing the Rust binary, (2) decoding sample files from a directory, (3) changing CLI behavior, metadata handling, or output rules, or (4) validating the tool against real input files."
---

# Audio Unpack CLI

## Overview

Use this skill when working on the local Rust decoder project in `E:\Projects\github\ncmdumpGUI-1.2`.
Treat the Rust CLI as the source of truth. Do not recreate the old C# project or reintroduce GUI code.

## Project Workflow

Set the working directory to `E:\Projects\github\ncmdumpGUI-1.2`.

Use these commands as the default workflow:

```powershell
cargo check
cargo build --release
cargo run --release -- --help
```

Use this command shape for real decoding runs:

```powershell
cargo run --release -- "<INPUT_DIR>" "<OUTPUT_DIR>" --verbose
```

Prefer testing against a separate output directory instead of writing back into the source tree.

## Implementation Rules

Keep the project as a standalone Rust CLI with `Cargo.toml`, `Cargo.lock`, and `src/main.rs`.
Preserve the current CLI contract unless the user explicitly asks to change it:

- positional `INPUT_DIR` and `OUTPUT_DIR`
- optional `--overwrite`
- optional `--strict-metadata`
- optional `--verbose`
- optional `--jobs`

Keep file processing file-level parallel and single-file decoding stream-based.
Preserve tolerant metadata handling by default: decode audio first, downgrade tag-writing issues to warnings unless strict mode is requested.
Keep public naming neutral and avoid platform- or vendor-specific marketing language in README, package metadata, and help text.

## Validation

Run at least `cargo check` after code changes.
Run `cargo build --release` before reporting the tool as ready.
When the user provides a real input directory, perform one end-to-end decode run into a dedicated output directory and report:

- success count
- skipped count
- failed count
- warning count

If outputs are created, list the generated files and their target directory.

## Notes

The current implementation supports writing tags for MP3 and FLAC.
If another output format is decoded, keep audio output working even if tag writing must be skipped with a warning.
