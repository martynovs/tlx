# Security policy

## Reporting a vulnerability

Use the repository's **Security > Report a vulnerability** entry to send a private report when
that option is available. Do not post exploit details, sensitive specifications, credentials or
private traces in a public issue.

If private reporting is unavailable, open an issue titled **Request for a private security
contact** with no vulnerability details and ask a maintainer for a private reporting channel.
Share the technical report only after that channel has been agreed. Do not send sensitive
information to an address inferred from a commit or to an unverified domain.

A useful report includes:

- The affected tlx version or commit, operating system and architecture.
- The command and relevant configuration, with credentials and private paths removed.
- A minimal specification, configuration and input data that reproduce the problem.
- Expected and observed behavior, impact, and any workaround you have verified.

For a suspected false success, include the property that should fail and a small counterexample
or TLC comparison when available. Avoid submitting proprietary models without permission.
Ordinary correctness bugs with non-sensitive reproducers can be reported as normal issues.

## Supported code

Security fixes target the current `main` branch and, when releases are available, the latest
published release. Older revisions have no guaranteed backport support. Until a fix is released,
maintainers may recommend a patched revision or a workaround. This project does not promise a
fixed response or resolution time.

## Running specifications safely

Treat specifications, imported modules and trace files as untrusted inputs unless their source is
trusted. Model checking can consume substantial CPU, memory and disk space; native operators can
read or write files, and compiled execution builds and loads native code. tlx is not a sandbox.

Run untrusted models in an isolated environment with explicit resource limits, a restricted
filesystem and no application credentials. Use a trusted Rust toolchain and C compiler, and keep
their versions and dependencies updated. A successful model check establishes only what the
configured model and properties express; it does not replace review of the model or the checker.

## Coordinating disclosure

Keep reports private while a fix is being prepared. Maintainers and reporters should agree on
publication of the report, reproducer and credit. Do not include personal or confidential details
in the eventual advisory without permission.
