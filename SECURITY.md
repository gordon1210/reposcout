# Security policy

## Supported versions

Security fixes are provided for the latest released version of RepoScout.

| Version | Supported |
|---|---|
| Latest release | ✅ |
| Older releases | ❌ |
| Unreleased development builds | ❌ |

## Reporting a vulnerability

Please do not publish sensitive vulnerability details in a public issue.

Use [GitHub's private vulnerability reporting][private-reporting] and select
**Report a vulnerability**. Include the affected version, expected and observed behavior,
reproduction steps or a proof of concept, and the potential impact when those details are
available.

If private reporting is unavailable, open a public issue that asks for a private contact
channel without including exploit details, sensitive data, or a full reproduction.

You should receive an acknowledgement within seven days. Confirmed vulnerabilities will be
handled through a private GitHub security advisory until a fix and coordinated disclosure are
ready.

## Daemon access

`reposcout daemon` binds to loopback and requires a fresh bearer token by default. The token is
stored in a user-scoped runtime/cache file for local tooling such as the Vite proxy, with
owner-only permissions on Unix; Host and Origin validation provide an additional browser and
DNS-rebinding boundary. Unauthenticated mode is an explicit loopback-only override. Non-loopback
plain HTTP requires
`--allow-insecure-remote`, remains authenticated, and should run only behind a TLS proxy.

## Release integrity

Release installers require successful SHA-256 verification before extracting a platform archive.
`reposcout update` independently downloads and verifies the immutable platform archive without
executing a downloaded installer script. GitHub build-provenance attestations can be verified with:

```sh
gh attestation verify PATH_TO_ARCHIVE --repo gordon1210/reposcout
```

The `curl | sh` installer is a convenience path that trusts GitHub TLS, this repository's release
permissions, and the release workflow because the downloaded script executes before its archive
verification. Security-sensitive installations should use the complete
[verified installation](docs/getting-started.md#verified-installation) procedure, which verifies
the immutable release, installer asset, workflow provenance, and source tag before execution.

[private-reporting]: https://github.com/gordon1210/reposcout/security/advisories
