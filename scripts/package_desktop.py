#!/usr/bin/env python3
"""Create a deterministic, self-contained Crownlines desktop archive."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import stat
import tomllib
import zipfile


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
FIXED_TIMESTAMP = (2020, 1, 1, 0, 0, 0)


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output-dir", default=Path("dist"), type=Path)
    parser.add_argument("--expected-version")
    return parser.parse_args()


def package_version() -> str:
    with (REPOSITORY_ROOT / "Cargo.toml").open("rb") as cargo_toml:
        manifest = tomllib.load(cargo_toml)
    return manifest["workspace"]["package"]["version"]


def archive_entry(archive: zipfile.ZipFile, source: Path, destination: str) -> None:
    info = zipfile.ZipInfo(destination, FIXED_TIMESTAMP)
    mode = source.stat().st_mode
    info.external_attr = ((stat.S_IMODE(mode) | stat.S_IFREG) & 0xFFFF) << 16
    info.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(info, source.read_bytes())


def text_entry(archive: zipfile.ZipFile, destination: str, content: str) -> None:
    info = zipfile.ZipInfo(destination, FIXED_TIMESTAMP)
    info.external_attr = (0o100644 & 0xFFFF) << 16
    info.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(info, content.encode("utf-8"))


def iter_files(root: Path) -> list[Path]:
    return sorted(path for path in root.rglob("*") if path.is_file())


def main() -> None:
    args = arguments()
    version = package_version()
    if args.expected_version and args.expected_version.removeprefix("v") != version:
        raise SystemExit(
            f"release tag {args.expected_version!r} does not match Cargo version {version!r}"
        )
    if not args.binary.is_file():
        raise SystemExit(f"release binary does not exist: {args.binary}")

    required_files = [
        REPOSITORY_ROOT / "LICENSE-APACHE",
        REPOSITORY_ROOT / "LICENSE-MIT",
        REPOSITORY_ROOT / "THIRD_PARTY_NOTICES.md",
        REPOSITORY_ROOT / "README.md",
        REPOSITORY_ROOT / "docs" / "player-guide.md",
        REPOSITORY_ROOT / "docs" / "privacy.md",
    ]
    assets = REPOSITORY_ROOT / "assets"
    missing = [str(path) for path in [*required_files, assets] if not path.exists()]
    if missing:
        raise SystemExit(f"required package inputs are missing: {', '.join(missing)}")

    root_name = f"crownlines-{version}-{args.target}"
    args.output_dir.mkdir(parents=True, exist_ok=True)
    archive_path = args.output_dir / f"{root_name}.zip"
    executable_name = "crownline.exe" if args.binary.suffix == ".exe" else "crownline"

    with zipfile.ZipFile(archive_path, "w") as archive:
        archive_entry(archive, args.binary, f"{root_name}/{executable_name}")
        for source in required_files:
            archive_entry(archive, source, f"{root_name}/{source.relative_to(REPOSITORY_ROOT)}")
        for source in iter_files(assets):
            archive_entry(archive, source, f"{root_name}/{source.relative_to(REPOSITORY_ROOT)}")
        text_entry(
            archive,
            f"{root_name}/BUILD-INFO.txt",
            f"version={version}\ntarget={args.target}\nrevision={args.revision}\n",
        )

    digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path = archive_path.with_suffix(f"{archive_path.suffix}.sha256")
    checksum_path.write_text(f"{digest}  {archive_path.name}\n", encoding="utf-8")
    print(archive_path)


if __name__ == "__main__":
    main()
