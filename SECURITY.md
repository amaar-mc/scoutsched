# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

Do NOT open a public GitHub issue for security vulnerabilities.

Send a private report to: amaardevx@gmail.com

Include:
- A description of the vulnerability
- Steps to reproduce it
- The version of scoutsched affected
- Any proposed fix if you have one

You will receive an acknowledgement within 48 hours and a resolution or status
update within 7 days.

## Scope

scoutsched is a CLI tool that reads local files and optionally fetches data from
The Blue Alliance API. The primary security concerns are:

- API key handling: the TBA key is accepted only via `--tba-key` flag or the
  `TBA_API_KEY` environment variable. It is never written to disk or logged.
- Input validation: malformed or adversarially crafted matches JSON or TOML
  config files. The parser validates all inputs before the solver runs.
- Dependency supply chain: tracked via `Cargo.lock`. Report any known-vulnerable
  transitive dependency.
