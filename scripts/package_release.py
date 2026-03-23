#!/usr/bin/env python3
from __future__ import annotations

import argparse
import pathlib
import shutil
import zipfile


def main() -> int:
    parser = argparse.ArgumentParser(description="Package a release archive for audio-unpack-cli.")
    parser.add_argument("--target", required=True, help="Target triple label used in archive name.")
    parser.add_argument(
        "--binary",
        default=None,
        help="Optional path to built binary. Defaults to target/release/audio-unpack-cli(.exe).",
    )
    args = parser.parse_args()

    repo_root = pathlib.Path(__file__).resolve().parent.parent
    release_dir = repo_root / "target" / "release"
    binary_name = "audio-unpack-cli.exe" if shutil.which("powershell") else "audio-unpack-cli"
    binary_path = pathlib.Path(args.binary) if args.binary else release_dir / binary_name

    if not binary_path.exists():
        raise SystemExit(f"Binary not found: {binary_path}")

    dist_dir = repo_root / "dist"
    dist_dir.mkdir(exist_ok=True)

    archive_name = f"audio-unpack-cli-{args.target}.zip"
    archive_path = dist_dir / archive_name

    readme = repo_root / "README.md"
    license_file = repo_root / "LICENSE"
    skill_dir = repo_root / "skills" / "audio-unpack-cli"

    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.write(binary_path, arcname=binary_path.name)
        if readme.exists():
            zf.write(readme, arcname="README.md")
        if license_file.exists():
            zf.write(license_file, arcname="LICENSE")
        if skill_dir.exists():
            for path in skill_dir.rglob("*"):
                if path.is_file():
                    zf.write(path, arcname=str(path.relative_to(repo_root)))

    print(archive_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
