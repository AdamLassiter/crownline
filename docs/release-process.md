# Desktop release process

Desktop archives are built only from a semantic version tag matching
`vMAJOR.MINOR.PATCH`. The tag version must equal `[workspace.package].version`
in `Cargo.toml`; otherwise packaging stops before publication.

## Archive contract

Every target produces a deterministic ZIP named
`crownlines-VERSION-TARGET.zip`. Its single top-level directory contains:

- the native `crownline` executable (`crownline.exe` on Windows);
- the complete `assets` tree, including scenarios, chess font, provenance, and
  the SIL Open Font License;
- project MIT and Apache-2.0 licenses and third-party notices;
- README, player guide, and privacy statement;
- `BUILD-INFO.txt` with application version, target triple, and full source
  revision.

Settings, credentials, saves, build intermediates, and other user/development
state are never package inputs. `scripts/package_desktop.py` uses fixed archive
timestamps and sorted assets, then writes a sibling SHA-256 file. The release
job validates every checksum, publishes a combined `SHA256SUMS`, and attaches a
GitHub build-provenance attestation to each ZIP.

At runtime the client first resolves `assets` beside its own executable, then
falls back to the repository-relative path used by development builds. The
archive can therefore be launched from another working directory without losing
its scenarios or font.

For a local package after `cargo build --locked --release -p crownline`:

```sh
python3 scripts/package_desktop.py \
  --binary target/release/crownline \
  --target x86_64-unknown-linux-gnu \
  --revision "$(git rev-parse HEAD)"
```

Unpack the result and run `crownline --version` from outside the repository to
verify the clean layout without starting the graphical client. The same version
and abbreviated revision are visible in the game window title.

## CI targets and trust boundary

`.github/workflows/release-desktop.yml` builds native Linux x86-64, Windows
x86-64, macOS Intel, and macOS Apple Silicon clients. Each fresh runner unpacks
its archive and runs the packaged binary's `--version` path before upload. The
workflow is tag-only; it is not callable from pull requests and its default
permissions are read-only. The publication job alone receives release-content
write permission.

Optional signing credentials are release-environment secrets:

- Windows: `WINDOWS_SIGNING_CERTIFICATE` (base64 PFX) and
  `WINDOWS_SIGNING_PASSWORD`.
- macOS: `APPLE_SIGNING_CERTIFICATE` (base64 PKCS#12),
  `APPLE_SIGNING_PASSWORD`, and `APPLE_SIGNING_IDENTITY`.
- macOS notarization additionally requires `APPLE_ID`, `APPLE_APP_PASSWORD`,
  and `APPLE_TEAM_ID`.

When the corresponding certificate exists, signing occurs before packaging.
When the complete Apple account set also exists, the signed ZIP is submitted to
Apple's notary service and publication waits for success. Missing credentials
produce an explicitly unsigned archive; they never fall back to values from a
pull-request context. Signing/notarization status must be stated in release
notes so players can distinguish unsigned development releases.
