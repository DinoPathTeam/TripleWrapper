# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 1.0.x   | ✅        |
| < 1.0   | ❌        |

## Scope

TripleWrapper runs entirely locally and executes system tools (7z, tar,
pixz) and, optionally, third-party plugin executables found on your
`PATH`. Reports about a malicious file already on your `PATH` or a
compromised local user account are out of scope — TripleWrapper trusts
its execution environment the same way any local CLI tool does.

In-scope examples: command injection via archive paths/filenames,
path traversal on extract, password leakage (logs, process list,
persisted state), unsafe handling of the local device-mount flow.

## Reporting a Vulnerability

Please do not open public issues for security problems. Use
**Security → Report a vulnerability** (private advisories) and we
will acknowledge receipt within 5 business days.

Do not disclose details publicly until a fix is released.
