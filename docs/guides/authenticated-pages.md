# Capture authenticated pages

Offprint accepts request headers and browser cookies from protected JSON files.
The files keep secret values out of command arguments and process listings.

The resulting artifact can contain private page content. Apply the source
system's access, retention, and sharing rules to the HTML artifact and every
export derived from it.

## Pass request headers

Create `headers.json`:

```json
{
  "Authorization": "Bearer token"
}
```

Protect and use it:

```console
chmod 600 headers.json
offprint capture https://example.com/account \
  --headers headers.json \
  --output account.html
```

Headers can also use a list of `{ "name", "value" }` objects. Offprint rejects
duplicate names, browser-controlled headers, and values containing header
control characters. At most 256 entries are accepted.

Custom headers apply to requests with the same scheme, host, and effective port
as the capture URL. A cross-origin image or frame does not receive the
authorization header.

## Pass browser cookies

Create `cookies.json`:

```json
{
  "cookies": [
    {
      "name": "session",
      "value": "secret",
      "url": "https://example.com/",
      "secure": true,
      "httpOnly": true,
      "sameSite": "lax"
    }
  ]
}
```

```console
chmod 600 cookies.json
offprint capture https://example.com/account \
  --cookies cookies.json \
  --output account.html
```

Each cookie needs a URL or domain. URL scope accepts HTTP or HTTPS. Domain scope
must match the capture host or one of its parent domains. Offprint validates
cookie destinations through the selected network policy before navigation. A
request accepts at most 4,096 cookies, with names and values limited to 4,096
bytes each.

## File and stream rules

Credential files must be regular files no larger than 1 MiB. Unix group and
other permissions must be clear. Windows access control entries must not grant
access to unrelated principals.

One credential input can use standard input through `--headers -` or
`--cookies -`. Header and cookie inputs cannot both consume standard input.
Raw artifact output through `--output -` cannot share the same stream.

`SecretString` values serialize as `[REDACTED]`. A serialized capture request is
therefore an audit record, not a replayable credential store. Supply fresh
secrets when replaying a request.

Password input values are redacted from captured state by default. Set
`preserve_password_values = true` in a capture profile only when the saved
artifact must contain them, then protect the artifact as credential-bearing
data.

## Diagnose missing authenticated resources

The commands use [jq](https://jqlang.org/), a command-line JSON query tool.

Use JSON output and inspect the aggregate counts:

```console
offprint capture https://example.com/account \
  --headers headers.json \
  --output account.html \
  --json > capture.json

jq '.resources, .warnings' capture.json
```

Inspect the artifact manifest for the failed reference and retrieval source:

```console
offprint artifact inspect account.html --json > manifest.json
jq '.resourceRecords[] | select(.outcome.kind == "failed")' manifest.json
```

A failed cross-origin asset may require a correctly scoped cookie rather than a
same-origin header. Preserve the original artifact and diagnostic bundle when
the failure remains unexplained.
