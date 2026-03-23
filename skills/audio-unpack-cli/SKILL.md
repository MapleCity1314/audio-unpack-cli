---
name: audio-unpack-cli
description: "Build, run, test, and modify an audio-unpack-cli Rust repository. Use when Codex needs to work on the command-line audio container decoder itself, especially for: (1) building or releasing the Rust binary, (2) decoding sample files from a directory, (3) changing CLI behavior, metadata handling, or output rules, (4) validating the tool against real input files, or (5) packaging release artifacts and release automation."
---

# Audio Unpack CLI

## Overview

Use this skill when working on an `audio-unpack-cli` Rust repository.
Treat the Rust CLI as the source of truth. Do not recreate the old C# project or reintroduce GUI code.

## Project Workflow

Set the working directory to the repository root that contains `Cargo.toml`, `src/main.rs`, and optionally `skills/audio-unpack-cli/`.

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

For release packaging, use:

```powershell
python .\scripts\package_release.py --target windows-x86_64-pc-windows-msvc
```

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
Keep bundled skill content path-agnostic so it works across machines and operating systems.

## Validation

Run at least `cargo check` after code changes.
Run `cargo build --release` before reporting the tool as ready.
If release workflow or packaging changes, also verify the release archive script and generated archive name.
When the user provides a real input directory, perform one end-to-end decode run into a dedicated output directory and report:

- success count
- skipped count
- failed count
- warning count

If outputs are created, list the generated files and their target directory.

## Notes

The current implementation supports writing tags for MP3 and FLAC.
If another output format is decoded, keep audio output working even if tag writing must be skipped with a warning.
