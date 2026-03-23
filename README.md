# audio-unpack-cli

Fast cross-platform CLI for unpacking supported audio container files.

## Features

- Recursive batch scan for supported input files
- Parallel file-level decoding
- Output to a specified directory while preserving relative paths
- Safe-by-default skip behavior for existing outputs
- Metadata writing for MP3 and FLAC
- Tolerant metadata parsing with optional strict mode
- Bundled Codex skill for working on this repo

## Build

```powershell
cargo build --release
```

To create a distributable archive locally:

```powershell
python .\scripts\package_release.py --target windows-x86_64-pc-windows-msvc
```

## Usage

```powershell
.\target\release\audio-unpack-cli.exe <INPUT_DIR> <OUTPUT_DIR> [--overwrite] [--strict-metadata] [--verbose] [--jobs N]
```

Example:

```powershell
.\target\release\audio-unpack-cli.exe "C:\InputAudio" "C:\DecodedAudio"
```

## Notes

- MP3 and FLAC tags are written when metadata is available.
- Unsupported output formats still decode audio, but tag writing is skipped with a warning.
- A matching Codex skill is included at `skills/audio-unpack-cli/`.
- GitHub tag releases can be built automatically with the workflow in `.github/workflows/release.yml`.
