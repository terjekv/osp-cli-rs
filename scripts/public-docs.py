#!/usr/bin/env python3
"""Enforce compiler-backed public Rustdoc coverage for the library.

Repo-wide checks compile the working tree. The pre-commit --staged mode
exports the index before compiling, so unstaged changes cannot weaken the
hook's contract. Public item coverage belongs to rustc, not a source parser.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile


def repo_root() -> pathlib.Path:
    """Anchor git operations to the repository that owns this script."""

    return pathlib.Path(__file__).resolve().parent.parent


def is_internal_test_module(path: pathlib.Path) -> bool:
    """Keep internal test-only modules outside the public docs contract."""

    posix_path = path.as_posix()
    return posix_path.endswith("/tests.rs") or "/tests/" in posix_path


def staged_rust_files(root: pathlib.Path) -> list[pathlib.Path]:
    """Return staged production Rust files under `src/`."""

    cmd = [
        "git",
        "diff",
        "--cached",
        "--name-only",
        "--diff-filter=ACMR",
        "--",
        "src/**/*.rs",
        "src/*.rs",
    ]
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        check=True,
        cwd=root,
    )
    return sorted(
        pathlib.Path(raw.strip())
        for raw in result.stdout.splitlines()
        if raw.strip() and not is_internal_test_module(pathlib.Path(raw.strip()))
    )


def compiler_missing_docs_failures(root: pathlib.Path, cwd: pathlib.Path) -> list[str]:
    """Return compiler-backed missing-docs failures for the library crate."""

    env = os.environ.copy()
    env.setdefault("CARGO_TARGET_DIR", str(root / "target" / "public-docs"))
    result = subprocess.run(
        [
            "cargo",
            "rustc",
            "--lib",
            "--locked",
            "--message-format",
            "short",
            "--",
            "-Dmissing-docs",
        ],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode == 0:
        return []

    diagnostics = [
        line
        for line in (result.stdout.splitlines() + result.stderr.splitlines())
        if line.strip()
    ]
    if not diagnostics:
        diagnostics = ["`cargo rustc --lib --locked -- -Dmissing-docs` failed without diagnostics."]
    return [
        "compiler-backed public Rustdoc coverage check failed:",
        *[f"  {line}" for line in diagnostics],
    ]


def export_staged_index(root: pathlib.Path) -> pathlib.Path:
    """Export the staged git index into a temporary directory."""

    target_root = root / "target"
    target_root.mkdir(parents=True, exist_ok=True)
    temp_root = pathlib.Path(
        tempfile.mkdtemp(prefix="osp-public-docs-", dir=target_root)
    )
    subprocess.run(
        [
            "git",
            "checkout-index",
            "--all",
            "--force",
            f"--prefix={temp_root}{os.sep}",
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return temp_root


def check_staged_public_docs(root: pathlib.Path) -> list[str]:
    """Evaluate the public docs contract against the staged git index."""

    files = staged_rust_files(root)
    if not files:
        return []

    failures: list[str] = []
    staged_root = export_staged_index(root)
    try:
        failures.extend(compiler_missing_docs_failures(root, staged_root))
    finally:
        shutil.rmtree(staged_root, ignore_errors=True)
    return failures


def main() -> int:
    """Parse CLI mode and run the public docs contract check."""

    parser = argparse.ArgumentParser(
        description=(
            "Check compiler-backed public Rustdoc coverage."
        )
    )
    parser.add_argument(
        "--staged",
        action="store_true",
        help="Check the staged git index, intended for pre-commit hooks.",
    )
    args = parser.parse_args()

    root = repo_root()
    if args.staged:
        failures = check_staged_public_docs(root)
    else:
        failures = compiler_missing_docs_failures(root, root)
    if not failures:
        return 0

    if args.staged:
        print(
            "ERROR: staged public Rust items must satisfy the public Rustdoc contract.",
            file=sys.stderr,
        )
    else:
        print(
            "ERROR: public Rust items must satisfy the public Rustdoc contract.",
            file=sys.stderr,
        )
    print(file=sys.stderr)
    for failure in failures:
        print(f"  {failure}", file=sys.stderr)
    print(file=sys.stderr)
    print(
        "Add a `///` Rustdoc comment above the exported item.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
