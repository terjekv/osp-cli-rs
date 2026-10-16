# Cookbook

This file is a grab bag of small, copyable patterns.

Unless noted otherwise, every example here uses upstream command surfaces that
exist in a plain `osp-cli` install.

Examples that use `inventory ...` are illustrative provider-backed command
shapes. Replace them with a real plugin command from `osp plugins list`.

## Inspect Plugin-Provided Commands

```bash
osp plugins list
osp plugins list
```

## Ask For JSON Once

```bash
osp plugins list --json
```

## Force Plain JSON For Scripts

```bash
osp --format json --render-mode plain plugins list
```

## Explain Why A Config Value Won

```bash
osp config explain ui.format
osp config get ui.format --sources
```

## Show The Whole Resolved Config

```bash
osp config show
osp config show --sources
```

## Start A Short REPL Session

```text
plugins list
plugins list --format md
help config
```

## Ask For Help In Guide Form

```bash
osp help --format auto
```

```text
help config
```

## Switch Output Format For One Command

```bash
osp plugins list --format md
osp plugins list --value
```

## Inspect Plugin Search Order

```bash
osp plugins list
osp doctor plugins
```

Builtins and native commands win; the first matching executable wins among
plugins. Remove a shadowing copy or change the search path to choose another.

## Set Sane Daily Defaults

```bash
osp config set ui.presentation compact --permanent
osp config set ui.format table --permanent
osp config set repl.simple_prompt true --permanent
```

## Do A First-Pass Plugin Health Check

```bash
osp doctor plugins
osp -d doctor plugins
```

## Keep Reading

- guided first session:
  [GETTING_STARTED.md](GETTING_STARTED.md)
- REPL behavior:
  [REPL.md](REPL.md)
- formatting and output:
  [FORMATTING.md](FORMATTING.md)
- troubleshooting:
  [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
