# Release signing

Version0.4.0 introduces a project-owned **self-signed** publisher identity. It is not a publicly trusted certificate, Microsoft reputation approval, Apple Developer identity or notarization. Windows/macOS warnings can remain. No root certificate or machine trust is installed by the project.

## Verify an archive

Obtain the public key from a previously trusted checkout or independently checked release/source identity. A key fetched only beside an unknown archive does not establish trust. The committed certificate and SHA256 pin are in `resources/signing/publisher-cert.pem` and `resources/signing/publisher.json`.

Using an already installed OpenSSL, verify the matching archive and detached signature:

```bash
openssl dgst -sha256 -verify resources/signing/publisher-public.pem -signature numera-cha-0.4.0-windows-x64.zip.sig numera-cha-0.4.0-windows-x64.zip
```

The command must report `Verified OK` and exit0. Then inspect `package-manifest.json`: expected version, platform, exact source commit, entry names/sizes/SHA256, matching source and independent notices. Reject altered files or a different signer. Keep the working installation and private state until the replacement is verified.

Windows executables and the NSIS installer additionally have embedded Authenticode signatures. Use `scripts/verify_windows_signature.ps1 -Path <executable>` from a trusted source checkout with PowerShell7: it verifies the pinned certificate, CMS signature and SHA256 PE image digest without installing trust. `Get-AuthenticodeSignature` alone can report `NotTrusted` or `UnknownError` for this self-signed identity; neither status establishes integrity or permits ignoring altered bytes or another signer. macOS code signatures identify this owner certificate but are not notarized. Linux uses the signed archive and member manifest rather than claiming an embedded executable signature.

## Custody and builds

Only the public certificate/key/pin are distributed. Private key, export container and password remain in owner-restricted local custody and the owning repository's named Actions secrets. Protected inputs are materialized only in the hosted job's private work directory and removed after the job. They are never source, logs, public arguments, caches or release artifacts. Signing does not change the license or independent third-party notices.

Manual CI builds/signs/validates final source; manual promotion reuses exact archives without rebuilding. Native and detached checks must pass before upload/promotion. Wrong-key and tampered-artifact checks must fail. There is no automatic signer replacement, paid signing service, certificate purchase, root-store change or online notarization.

No timestamp authority is claimed. Expired/revoked or replaced identities need an explicit release decision and independently verified public-key continuity; do not silently accept a new key.
