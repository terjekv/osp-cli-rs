#!/usr/bin/env python3
"""Best-effort Click/Typer -> osp command metadata helper.

This inspects Click command objects directly. Typer apps work because they
compile down to Click commands. The output is a convenience starter, not a
guaranteed canonical translation.
"""

from __future__ import annotations

import argparse
import importlib
import json
import sys
from typing import Any

import click


def load_attr(spec: str) -> Any:
    module_name, sep, attr_name = spec.partition(":")
    if not sep:
        raise SystemExit("expected MODULE:ATTR")
    module = importlib.import_module(module_name)
    return getattr(module, attr_name)


def convert_param(param: click.Parameter) -> tuple[str, dict[str, Any]] | dict[str, Any]:
    suggestions = []
    if isinstance(param.type, click.Choice):
        suggestions = [{"value": str(choice)} for choice in param.type.choices]

    if isinstance(param, click.Option):
        names = list(param.opts) + list(param.secondary_opts)
        long_flags = [name for name in names if name.startswith("--")]
        key = long_flags[0] if long_flags else names[0]
        return key, {
            "about": param.help,
            "flag_only": not getattr(param, "is_flag", False) and param.nargs == 0 or getattr(param, "is_flag", False),
            "multi": bool(param.multiple),
            "value_type": "Path" if isinstance(param.type, click.Path) else None,
            "suggestions": suggestions,
        }

    return {
        "name": param.name,
        "about": None,
        "multi": bool(getattr(param, "nargs", 1) != 1 or getattr(param, "multiple", False)),
        "value_type": "Path" if isinstance(param.type, click.Path) else None,
        "suggestions": suggestions,
    }


def convert_command(name: str, command: click.Command) -> dict[str, Any]:
    node = {
        "name": name,
        "about": command.help or command.short_help or "",
        "args": [],
        "flags": {},
        "subcommands": [],
    }

    for param in command.params:
        converted = convert_param(param)
        if isinstance(converted, tuple):
            key, payload = converted
            node["flags"][key] = payload
        else:
            node["args"].append(converted)

    if isinstance(command, click.MultiCommand):
        ctx = click.Context(command)
        for child_name in command.list_commands(ctx):
            child = command.get_command(ctx, child_name)
            if child is not None:
                node["subcommands"].append(convert_command(child_name, child))

    return node


def build_describe(command: click.Command) -> dict[str, Any]:
    payload = convert_command("", command)
    payload.pop("name")
    payload["output"] = "json"
    return payload


def main() -> int:
    cli = argparse.ArgumentParser(description="Convert a Click/Typer command into a optional describe skeleton.")
    cli.add_argument("command_ref", help="Python reference in MODULE:ATTR form")
    args = cli.parse_args()

    command = load_attr(args.command_ref)
    if not isinstance(command, click.Command):
        raise SystemExit("target is not a click.Command / Typer app")

    json.dump(build_describe(command), sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
