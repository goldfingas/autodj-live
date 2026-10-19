"""Policy tests for the optional AutoDJ Live workspace."""

from __future__ import annotations

import ast
import json
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _toml(relative_path: str) -> dict[str, object]:
    with (ROOT / relative_path).open("rb") as source:
        return tomllib.load(source)


def _json(relative_path: str) -> dict[str, object]:
    return json.loads((ROOT / relative_path).read_text(encoding="utf-8"))


def _literal_calls(source: str, function_name: str) -> set[tuple[str | None, ...]]:
    """Return positional calls without coupling policy to formatting or paths."""
    calls: set[tuple[str | None, ...]] = set()
    for node in ast.walk(ast.parse(source)):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name):
            continue
        if node.func.id != function_name:
            continue
        calls.add(
            tuple(
                argument.value
                if isinstance(argument, ast.Constant) and isinstance(argument.value, str)
                else None
                for argument in node.args
            )
        )
    return calls


def _literal_assignment(source: str, name: str) -> object:
    for node in ast.parse(source).body:
        if (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
            and node.targets[0].id == name
        ):
            return ast.literal_eval(node.value)
    raise AssertionError(f"{name} assignment is missing")


def test_live_workspace_pins_toolchain_and_keeps_python_package_optional() -> None:
    cargo = _toml("Cargo.toml")
    toolchain = _toml("rust-toolchain.toml")
    cargo_config = _toml(".cargo/config.toml")
    pyproject = _toml("pyproject.toml")

    assert cargo["workspace"]["members"] == [
        "crates/live-protocol",
        "crates/live-engine",
        "crates/live-supervisor",
        "apps/autodj-live/src-tauri",
    ]
    assert cargo["workspace"]["resolver"] == "2"
    assert cargo["profile"]["release"]["panic"] == "abort"
    assert toolchain["toolchain"]["channel"] == "1.99.1"
    assert toolchain["toolchain"]["profile"] == "minimal"
    assert set(toolchain["toolchain"]["components"]) == {"clippy", "rustfmt"}
    assert "x86_64-pc-windows-msvc" in toolchain["toolchain"]["targets"]
    assert cargo_config["build"]["target"] == "x86_64-pc-windows-msvc"

    python_dependencies = [*pyproject["project"].get("dependencies", [])]
    for group in pyproject["project"].get("optional-dependencies", {}).values():
        python_dependencies.extend(group)
    python_dependencies.extend(pyproject.get("build-system", {}).get("requires", []))
    assert all("rust" not in dependency.lower() for dependency in python_dependencies)
    assert all("tauri" not in dependency.lower() for dependency in python_dependencies)


def test_live_npm_scripts_are_namespaced() -> None:
    root_package = _json("package.json")
    live_package = _json("apps/autodj-live/package.json")

    existing_scripts = {
        "test": "vitest run",
        "test:watch": "vitest",
        "lint": (
            "eslint --max-warnings 0 src/autodj/static tests/jsmodules "
            "tests/playwright vitest.config.js eslint.config.js"
        ),
        "deadcode": "knip",
        "audit:transitions": "node tests/playwright/transition_audit.mjs",
        "audit:health": "node tests/playwright/health_audit.mjs",
        "audit:visual": "node tests/playwright/visual_audit.mjs",
        "audit:ci": ("npm run audit:health && npm run audit:transitions && npm run audit:visual"),
    }
    for name, command in existing_scripts.items():
        assert root_package["scripts"][name] == command
    assert root_package["workspaces"] == ["apps/autodj-live"]
    assert root_package["scripts"]["live:dev"] == "npm run dev --workspace @autodj/live"
    assert root_package["scripts"]["live:stage"] == "python scripts/stage_live_sidecar.py"
    assert root_package["scripts"]["live:build"] == (
        "npm run live:stage && npm run build --workspace @autodj/live"
    )
    assert root_package["scripts"]["live:test"] == "npm run test --workspace @autodj/live"
    assert root_package["scripts"]["live:check"] == "npm run check --workspace @autodj/live"
    assert live_package["name"] == "@autodj/live"
    assert live_package["devDependencies"]["@tauri-apps/cli"] == "2.12.3"
    assert live_package["devDependencies"]["vite"] == "8.3.4"


def test_live_node_policy_accepts_future_major_versions() -> None:
    live_package = _json("apps/autodj-live/package.json")

    assert live_package["engines"]["node"] == ">=26"
    node_check = live_package["scripts"]["check"]
    assert "Number(node.split('.')[0]) < 26" in node_check
    assert "Number(node.split('.')[0]) !== 26" not in node_check


def test_ci_has_windows_live_gate() -> None:
    """ADR-0014-01 and ADR-0014-04: Windows CI gates Live checks separately."""
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")

    assert "  live-windows:" in workflow
    live_job = workflow.split("  live-windows:", maxsplit=1)[1]
    assert "runs-on: windows-latest" in live_job
    assert 'channel: "1.99.1"' in live_job
    assert 'node-version: "26.12.0"' in live_job
    assert 'python-version: "3.14"' in live_job
    for command in (
        "uv run --frozen --no-sync python scripts/check_architecture_docs.py",
        "cargo fmt --check",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo test --workspace",
        "npm --workspace @autodj/live test",
        "npm --workspace @autodj/live run lint",
        "uv run pytest --no-cov tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py",
        "npm run live:build",
    ):
        assert command in live_job
    assert "AUTODJ_PYTEST_WORKERS" not in live_job
    assert "os: [ubuntu-latest, windows-latest]" in workflow


def test_live_bundle_stages_a_complete_locked_python_runtime() -> None:
    """ADR-0014-04: bundled sidecar runtime comes from a locked, isolated stage."""
    root_package = _json("package.json")
    live_config = _json("apps/autodj-live/src-tauri/tauri.conf.json")
    pyproject = _toml("pyproject.toml")
    stage_script = (ROOT / "scripts/stage_live_sidecar.py").read_text(encoding="utf-8")
    smoke_script = (ROOT / "scripts/smoke_live_sidecar_runtime.py").read_text(encoding="utf-8")

    assert root_package["scripts"]["live:stage"] == "python scripts/stage_live_sidecar.py"
    assert root_package["scripts"]["live:build"].startswith("npm run live:stage && ")
    assert live_config["bundle"]["resources"]["../../../target/live-sidecar/python"] == ("python")
    assert "../../../.venv/Scripts/python.exe" not in live_config["bundle"]["resources"]
    assert pyproject["dependency-groups"]["live-sidecar"] == [
        "click>=8.1.0",
        "rich>=13.0.0",
        "fastapi>=0.136.3",
        "uvicorn>=0.47.0",
    ]
    assert _literal_assignment(stage_script, "SIDECAR_MODULES") == (
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
    stage_commands = _literal_calls(stage_script, "_run")
    assert ("uv", "python", "install", "3.14") in stage_commands
    assert any(
        command[:2] == ("uv", "export")
        and "--only-group" in command
        and command[command.index("--only-group") + 1] == "live-sidecar"
        and "--no-emit-project" in command
        for command in stage_commands
    )
    assert any(
        command[:3] == ("uv", "pip", "install")
        and "--require-hashes" in command
        and "--break-system-packages" in command
        for command in stage_commands
    )
    assert "smoke_live_sidecar_runtime.py" in stage_script
    assert '"-I"' in smoke_script
    assert "AUTODJ_LIVE_TOKEN" in smoke_script
    assert "AUTODJ_LIVE_SESSION_ID" in smoke_script
