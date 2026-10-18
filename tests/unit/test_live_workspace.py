"""Policy tests for the optional AutoDJ Live workspace."""

from __future__ import annotations

import json
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _toml(relative_path: str) -> dict[str, object]:
    with (ROOT / relative_path).open("rb") as source:
        return tomllib.load(source)


def _json(relative_path: str) -> dict[str, object]:
    return json.loads((ROOT / relative_path).read_text(encoding="utf-8"))


def test_live_workspace_pins_toolchain_and_keeps_python_package_optional() -> None:
    cargo = _toml("Cargo.toml")
    toolchain = _toml("rust-toolchain.toml")
    cargo_config = _toml(".cargo/config.toml")
    pyproject = _toml("pyproject.toml")

    assert cargo["workspace"]["members"] == ["crates/live-protocol"]
    assert cargo["workspace"]["resolver"] == "2"
    assert cargo["profile"]["release"]["panic"] == "abort"
    assert toolchain["toolchain"]["channel"] == "1.99.1"
    assert toolchain["toolchain"]["profile"] == "minimal"
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
    assert root_package["scripts"]["live:build"] == "npm run build --workspace @autodj/live"
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
