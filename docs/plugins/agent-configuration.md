# Agent configuration declarations

English | [中文](agent-configuration.zh.md)

`ora-plugin-config::compile_agent_configuration` validates an immutable Agent declaration without
reading user configuration, running a process, or making a network request. It is a separate
compiler API. The installed configuration dispatcher also recognizes this shape, and package
validation compiles it for `agent`-kind packages. The local configuration store and its editor
serve the version-one shapes. Successful compilation is not installation, authorization, or
runtime readiness.

## Input

```json
{
  "schemaVersion": 2,
  "agent": {
    "modelAccess": {
      "contractVersion": 1,
      "protocols": ["anthropic-messages"],
      "modelCardinality": "single"
    }
  },
  "settings": {
    "verbose": {
      "type": "boolean",
      "title": "Verbose output",
      "description": "Include additional output detail",
      "default": false
    }
  }
}
```

- `schemaVersion` must be `2`; the model-access `contractVersion` must be `1`.
- The protocol list must be nonempty and contain no duplicates. Its exact names are
  `openai-completions` and `anthropic-messages`. Provider names such as `openai` and unsupported
  protocols such as `openai-responses` are rejected, not normalized to a supported protocol.
- `modelCardinality` must be `single`. The declaration cannot authorize an additional model.
- `settings` may be omitted. If present, it must be a nonempty object of the existing scalar
  Setting declarations; explicit `null` is not omission. Types remain string, number and boolean.
- Connection selections, API keys, runtime headers and upstream URLs do not belong in
  `agent.modelAccess`. Unknown members are rejected at every level. MCP `transport` and Hook
  contributions cannot be mixed into an Agent declaration.
- The existing 256 KiB source-size limit, 128-Setting limit, Setting IDs, text bounds, required
  flags, ordering and typed-default validation apply. Duplicate JSON keys are rejected before
  deserialization can collapse them.

Settings here are author-supplied metadata, not a secret store or a runtime expression language.
The compiler does not infer whether arbitrary text is a credential. Credential delivery and the
interpretation of supported non-secret options belong to separately authorized consumers.

## Compiled identity and errors

The result contains typed model capabilities and deterministically ordered Setting declarations.
Its `fingerprint` is the lowercase SHA-256 of the exact input bytes. Whitespace changes this
identity; do not reserialize a release copy before checking it against the package. Existing
version-one declaration fingerprints keep their canonicalized semantics.

Errors distinguish oversized or invalid JSON, duplicate keys, invalid structure/Settings,
unsupported schema/contract versions, and empty, repeated or unsupported protocols. Error display
does not include rejected strings or source snippets. Compilation does not prove source namespace,
release version/target, archive digest, package containment, personal ownership or live model access.

## Contract vectors

The [shared vectors](../../crates/plugin-config/tests/fixtures/agent-configuration-contract.json) carry
literal `source` strings and expected compiled objects or error categories. Source strings keep
the same bytes on LF and CRLF checkouts, so consumers can compare declaration identities without
depending on the checkout platform.

## Release association and install verification

A marketplace release manifest carries the publication association in a release-form-only
`[agent_configuration]` section: `schema_version`, `declaration_sha256` (the SHA-256 of the exact
declaration bytes every package of the release must contain), and `copy` (the registry-entry-relative
path of the static declaration copy). Only an `agent` listing with a release source may carry it;
the installed manifest form rejects it, as does a section with unknown keys. The copy path consists
of `/`-separated ASCII segments matching `[A-Za-z0-9][A-Za-z0-9._-]*`; empty or parent segments,
backslashes, escapes and absolute paths are rejected.

Both install paths verify the association against the package's own bytes before committing the
installation. The marketplace path derives the expectation from the listing; the Node path receives
it frozen on the install plan. The Controller preserves the plan's declaration version and digest
when mapping the Cloud contract into the Node protocol. The package's `assets/config.json` must
compile to the associated schema version and exact-byte digest, or the installer rejects the package
in staging. An Agent package may carry a declaration without an association as inert package metadata;
the declaration alone grants no personal configuration or model access.

## Decisions

- [Agent configuration declarations](https://github.com/ora-space/specs/blob/main/decisions/desktop/plugin/manifest/20261017-agent-configuration-declarations.md)
