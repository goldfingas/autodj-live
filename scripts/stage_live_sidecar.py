"""Stage the self-contained Python runtime bundled by AutoDJ Live.

The desktop application must not depend on the contributor's virtual environment.
This script copies a uv-managed CPython distribution, installs the lock-derived
sidecar dependency set into it, and then exercises that relocated runtime before
Tauri consumes it as a resource.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STAGE_ROOT = ROOT / "target" / "live-sidecar"
RUNTIME = STAGE_ROOT / "python"
REQUIREMENTS = STAGE_ROOT / "requirements.txt"
SIDECAR_MODULES = (
    "__init__.py",
    "__main__.py",
    "cli.py",
    "launcher.py",
    "live_sidecar.py",
    "live_sidecar_cli.py",
    "machine_tokens.py",
    "secret_files.py",
    "version.py",
)


def _run(*command: str) -> str:
    """Run a tooling command and return its stdout for a single-value query."""
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if completed.returncode:
        raise RuntimeError(
            f"{' '.join(command)} failed with exit code {completed.returncode}:\n"
            f"{completed.stderr.strip()}"
        )
    return completed.stdout.strip()


def _managed_python() -> Path:
    # ``uv python install`` makes the requested CPython release reproducible on
    # clean Windows CI workers; --managed-python prevents accidentally copying
    # the project .venv instead of its complete standard-library runtime.
    _run("uv", "python", "install", "3.14")
    executable = Path(_run("uv", "python", "find", "--managed-python", "3.14")).resolve()
    if executable.name.casefold() != "python.exe":
        raise RuntimeError(f"uv returned an unexpected Python executable: {executable}")
    return executable


def _copy_runtime(source_python: Path) -> None:
    source_root = source_python.parent
    if RUNTIME.exists():
        shutil.rmtree(RUNTIME)
    RUNTIME.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(source_root, RUNTIME, ignore=shutil.ignore_patterns("__pycache__"))

    # The sidecar is intentionally a small, source-only subset of this project;
    # installing the main project would pull its model/audio dependencies back
    # into the desktop bundle.
    package_destination = RUNTIME / "Lib" / "site-packages" / "autodj"
    package_destination.mkdir(parents=True)
    for module in SIDECAR_MODULES:
        shutil.copy2(ROOT / "src" / "autodj" / module, package_destination / module)
    with (ROOT / "pyproject.toml").open("rb") as source:
        project = tomllib.load(source)["project"]
    name = str(project["name"])
    version = str(project["version"])
    metadata = package_destination.parent / f"{name}-{version}.dist-info"
    metadata.mkdir()
    (metadata / "METADATA").write_text(
        f"Metadata-Version: 2.4\nName: {name}\nVersion: {version}\n",
        encoding="utf-8",
    )


def _install_locked_dependencies() -> None:
    _run(
        "uv",
        "export",
        "--frozen",
        "--format",
        "requirements-txt",
        "--only-group",
        "live-sidecar",
        "--no-emit-project",
        "--output-file",
        str(REQUIREMENTS),
    )
    _run(
        "uv",
        "pip",
        "install",
        "--python",
        str(RUNTIME / "python.exe"),
        "--require-hashes",
        "--break-system-packages",
        "--no-deps",
        "--requirement",
        str(REQUIREMENTS),
    )


def main() -> None:
    source_python = _managed_python()
    if STAGE_ROOT.exists():
        shutil.rmtree(STAGE_ROOT)
    _copy_runtime(source_python)
    _install_locked_dependencies()
    subprocess.run(
        [sys.executable, "scripts/smoke_live_sidecar_runtime.py", str(RUNTIME)],
        cwd=ROOT,
        check=True,
    )
    print(f"staged AutoDJ Live sidecar runtime at {RUNTIME}")


if __name__ == "__main__":
    main()
