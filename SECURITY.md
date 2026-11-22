# Security policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's
["Report a vulnerability"](../../security/advisories/new) form on this repository, not in a
public issue. Include what you found, how to reproduce it and the affected version. We aim to
acknowledge a report within a few days.

## What is in scope

- The plugin sandbox (`crates/ps-device`): a plugin that escapes its resource limits, reads or
  writes files, reaches a bus or host it was not approved for, or hangs the host.
- Parsers of untrusted files (SVG, DXF, PDF/AI, `.pstudio`, `.psjob`, `.psdevice`, library
  packages): crashes, unbounded memory or time, path traversal.
- The update and material-index checks (`ps-net`): signature or pinning bypasses.
- The automation (MCP) API: any way to move hardware without a person passing `--arm-api`.

Machines and their firmware are out of scope; report those to their vendor.
