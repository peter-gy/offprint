# Manage local browsers

Offprint can discover a compatible system browser or install a catalog-pinned
managed browser. Use `doctor` first to inspect selection and recovery.

```console
offprint doctor
offprint browser list --json > browsers.json
```

## Install the managed revision

```console
offprint browser install
```

The default revision comes from Offprint's trusted browser catalog. Installation
downloads a bounded archive, verifies its SHA-256 digest, rejects unsafe archive
paths and entries, probes the executable version, and commits the staged cache
directory. Concurrent installers share a cache lock and reuse a complete valid
installation.

An explicit `--revision` must already exist in the catalog.

## Understand browser candidates

The local inventory reports:

- Product and version
- Source: managed, system, or explicit
- Candidate state: selected or compatible
- Selection priority and reason code
- Executable path or redacted endpoint
- Managed revision and active lease count when applicable

Automatic discovery chooses the first compatible candidate after applying the
configured `BrowserSourcePolicy`. `doctor` can also report remote and shadowed
candidates for its resolved service configuration.

## Remove a managed revision

```console
offprint browser remove REVISION
```

An active lease always blocks removal. The selected idle revision requires
`--force` and another compatible browser:

```console
offprint browser remove REVISION --force
```

Removal accepts a catalog revision identifier, not an arbitrary path. Offprint
renames the revision to a cache-local trash entry before deletion so discovery
does not observe a partial installation.

## Control automatic installation

Set browser policy in configuration:

```toml
[browser]
source = "managed"
installation = "install-managed"
```

Use `source = "managed"` to restrict automatic discovery to managed
candidates. An explicit service path or per-request executable still takes
precedence. Use `installation = "existing-only"` when provisioning belongs to the
host image or deployment system.

See [browser concepts](../concepts/browsers.md) for process and context
ownership.
