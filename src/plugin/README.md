# plugin

This module owns executable discovery and execution. An `osp-NAME` file is
one command; optional `--osp-describe` metadata adds completion, policy, and
JSON rendering. Native commands and builtins take precedence in the host.

- `manager.rs`: inventory, first-found lookup, shared command catalog types.
- `discovery.rs`: ordered roots, static marker scan, fingerprinted cache.
- `run.rs`: inherited stdio, bounded capture, describe timeout, signal handling.
- `env.rs`: command-scoped config environment projection.
- `catalog.rs`: recursive authorization metadata shared with natives.

The host chooses capture and lowers JSON into the existing output pipeline.
Wire types live in `core/plugin.rs`. See [the plugin guide](../../docs/PLUGINS.md)
and [the executable example](../../examples/plugins/osp-hello).
