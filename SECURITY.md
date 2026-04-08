# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly via [GitHub Security Advisories](https://github.com/denniskribl/oxicast/security/advisories/new). Do not open a public issue.

We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Scope

oxicast communicates with Cast devices over TLS on local networks. Key security considerations:

- **TLS certificate verification is disabled by default.** Cast devices use self-signed certificates. The connection is encrypted but not authenticated against a CA. This is standard practice across all Cast client implementations (pychromecast, go-chromecast, node-castv2). Enable `verify_tls(true)` on the builder if your device has a CA-signed certificate.
- **The `serve` feature binds an HTTP server on all interfaces** (`0.0.0.0`) with no authentication and `Access-Control-Allow-Origin: *`. This is intended for LAN-only use. Do not expose it to the internet.
- **Binary payloads and device authentication** (`urn:x-cast:com.google.cast.tp.deviceauth`) are not implemented.
