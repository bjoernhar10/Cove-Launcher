"""Builds the recommender as a single binary (PyInstaller) for Tauri's `externalBin`.

Output: apps/desktop/src-tauri/binaries/cove-recommender-<target-triple>[.exe]
Skips the build when sources, pyproject and lock file are unchanged (stamp file).

    uv run python scripts/build_sidecar.py [--force]
"""

from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT.parents[1] / "apps" / "desktop" / "src-tauri" / "binaries"
NAME = "cove-recommender"


def target_triple() -> str:
    triple = os.environ.get("TAURI_ENV_TARGET_TRIPLE")
    if triple:
        return triple
    output = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    for line in output.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit("could not determine the Rust target triple")


def source_hash() -> str:
    digest = hashlib.sha256()
    sources = sorted((ROOT / "src").rglob("*.py"))
    files = [*sources, ROOT / "pyproject.toml", ROOT / "uv.lock", Path(__file__).resolve()]
    for path in files:
        digest.update(path.relative_to(ROOT).as_posix().encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def main() -> None:
    triple = target_triple()
    suffix = ".exe" if "windows" in triple else ""
    target = OUT_DIR / f"{NAME}-{triple}{suffix}"
    stamp = OUT_DIR / f".{NAME}-{triple}.sha256"
    wanted = source_hash()
    up_to_date = target.is_file() and stamp.is_file() and stamp.read_text() == wanted
    if "--force" not in sys.argv and up_to_date:
        print(f"{target.name} is up to date")
        return

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            [
                sys.executable,
                "-m",
                "PyInstaller",
                "--onefile",
                "--noconfirm",
                "--clean",
                "--log-level=WARN",
                "--name",
                NAME,
                "--paths",
                str(ROOT / "src"),
                "--distpath",
                str(Path(tmp) / "dist"),
                "--workpath",
                str(Path(tmp) / "build"),
                "--specpath",
                tmp,
                str(ROOT / "src" / "cove_recommender" / "__main__.py"),
            ],
            check=True,
        )
        OUT_DIR.mkdir(parents=True, exist_ok=True)
        shutil.copy2(Path(tmp) / "dist" / f"{NAME}{suffix}", target)
    stamp.write_text(wanted)
    print(f"built {target} ({target.stat().st_size / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
