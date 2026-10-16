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

## Declared authentication

Plugins and native commands use `DescribeCommandAuthV1` directly. Unannotated
children inherit their nearest parent's requirements; an explicit child auth
object replaces that inheritance.

Supported visibility modes are `public`, `authenticated` (visible but requiring
an authenticated session to run) and `hidden`. A `run_session.credentials`
requirement can require a named credential to be `present` or `valid`:

```json
{
  "name": "apply",
  "auth": {
    "visibility": "public",
    "run_session": {
      "credentials": [{"state": "valid", "service": "product"}]
    }
  }
}
```

Product visibility and declared authentication both apply. Help only requires
visibility; execution additionally requires runnable access. Provider selection
and plugin enable/disable behavior remain independent of authentication.

Protocol-v1 DTOs still accept legacy `capability_gated`, `required_capabilities`,
`feature_flags`, `visible_session`, `auth_strength` and `fresh` credential
requirements. The host ignores these fields; `capability_gated` behaves as
`public`. There is no builtin policy hook, profile gate or policy override DSL.
Configuration allowlists `auth.visible.builtins` and `auth.visible.plugins` have
been removed. Use product visibility for external commands instead.

## Session diagnostics

`osp --json doctor` reads `session[0]` from the current hook. It retains `status`,
`authenticated`, `profile` and `credentials`. Each credential record contains
`service`, `valid` and `ttl_seconds` (or `null` when unknown):

```json
[{"service": "product", "valid": true, "ttl_seconds": 1800}]
```

The product computes validity and remaining lifetime; doctor does not acquire
credentials or age a cached snapshot. Human output formats the same facts.
`auth_strength`, `capability_count` and `enabled_features` are no longer emitted.
