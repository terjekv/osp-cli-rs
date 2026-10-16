# Authentication and command access

The library owns command discovery, visibility and dispatch. A product owns
identity, credential acquisition and service-specific session state. Backend
APIs remain authoritative for every operation.

## Product hook

Install one `App::with_access_policy(policy)` implementation of
`osp_cli::app::access::AccessPolicy`:

- `visibility(path)` returns `Visibility::Shown` or `Hidden(message)`. Paths use
  canonical command names. Hiding a parent hides its descendants in help,
  completion, REPL overviews and dispatch. An explicit hidden-command invocation
  reports the supplied message verbatim, or the standard hidden-policy message.
- `startup(runtime)` runs once after resolved configuration and runtime assembly,
  for both CLI and REPL. It does not run again after a REPL configuration rebuild.
- `before_run(path, runtime)` runs once for an external native or plugin execution,
  after visibility is checked and before declared runnable requirements. It never
  runs for builtins, help or completion. Products may acquire credentials here.
- `session_facts()` returns live `SessionFacts`: an authenticated flag and named
  `CredentialState` records. It must not prompt or perform acquisition. Access
  checks, doctor and prompt rendering read these facts directly.

All methods have public, tokenless defaults. The hook is `Send + Sync`, and the
same instance survives REPL rebuilds. Products should share their credential
owner with native integrations instead of copying tokens into the host.

Native dispatch invokes the execution hook before refreshing invocation metadata.
Help and completion never acquire credentials.

## Declared command policy

`CommandPolicyRegistry` evaluates command metadata with a `CommandPolicyContext`.
Products may still supply builtin policies and static context for profile,
capability, feature, strength and freshness requirements. Live authenticated and
credential facts from `AccessPolicy` replace those two fields during external
access checks. Product visibility is combined with declared visibility.

## Session diagnostics

Doctor reads current credentials from `session_facts()` without acquisition.
Credential records contain `service`, `valid` and `ttl_seconds`; existing session
summary fields remain available. Backend APIs remain authoritative.
