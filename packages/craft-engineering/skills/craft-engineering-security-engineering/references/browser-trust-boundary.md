# Treat the browser as an untrusted participant

Identify the trusted component that authorizes each protected read or write. Hidden buttons,
disabled fields and client-side route guards improve interaction but cannot enforce backend
permissions. Recheck resource and tenant access at the server boundary.

Follow external content to its rendering sink. Prefer the framework's text rendering; rich HTML
requires a maintained sanitizer and a deliberate allowlist. URL, stylesheet and script contexts
have different rules. Security headers can reduce exposure but do not replace correct handling.

Use the consumer's session threat model to choose storage and transport. Review script access,
CSRF exposure, cookie scope, expiry and refresh behavior. Do not invent a universal token-storage
rule from a single application. Never place credentials in telemetry or public bundle settings.

```text
Interaction check: user changes a resource identifier in a request.
Expected result: trusted authorization rejects access even if the UI offered no such control.
```

Assess cross-origin behavior using actual server and browser configuration, including credentialed
requests and redirects. Verify sensitive flows with realistic request identities and rendering;
a component mock can prove a button state but cannot prove server authorization or cookie policy.
