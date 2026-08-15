# Security policy

Please report vulnerabilities privately to the maintainers rather than opening a public issue.
Include affected versions, reproduction steps, impact, and a suggested remediation if known.

Ferrum treats request data and credentials as sensitive. Secret values must use `SecretStore`,
logs must pass through redaction, TLS verification remains enabled by default, and plugins or
remote services receive no capabilities without explicit user approval. Dependency advisories are
checked in CI. Supported security fixes target the latest minor release.
