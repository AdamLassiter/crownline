# Server image release policy

The Crownlines server is published only by
`.github/workflows/release-server.yml` after a trusted semantic version tag is
pushed. The `vMAJOR.MINOR.PATCH` tag must equal the workspace Cargo version.
Pull requests, branch pushes, and manually supplied image coordinates cannot
invoke publication.

## Coordinates and immutability

The workflow publishes to `ghcr.io/OWNER/REPOSITORY-server` with:

- `VERSION`, an immutable application-version tag;
- the full source commit SHA, an immutable revision tag;
- `stable`, a moving pointer updated only after all release gates pass.

Publication fails rather than replacing an existing version or revision tag.
Operators must deploy `IMAGE@sha256:DIGEST`; tags are discovery aids, not
rollback identity. OCI labels record title, description, version, source
revision, repository URL, and `MIT OR Apache-2.0` licensing.

## Required release gates

The exact locally built candidate is tested, scanned, tagged, and pushed; the
workflow does not rebuild between those steps. Builder and runtime base-image
manifest digests are pinned in the Dockerfile; changing either is a reviewed
source change, not an implicit rebuild-time update.

1. A fresh named Docker volume is mounted at `/var/lib/crownline`. The server
   must run as UID 10001, create a non-empty SQLite database, reach Docker
   `healthy`, and return success from `/health/ready`.
2. Trivy 0.70.0, installed by the commit-pinned Trivy Action v0.36.0, records
   complete reports for both the runtime image and locked source dependencies.
   Any HIGH or CRITICAL known vulnerability in either blocks publication,
   whether or not an upstream fix is listed.
3. The locked-source license scan blocks Trivy HIGH or CRITICAL
   restricted/forbidden findings. Lower-severity and unclassified findings
   remain visible in the report and must be reviewed before creating the trusted
   tag. Runtime OS-package licenses also remain in the image report for manual
   redistribution review. Crownlines is MIT OR Apache-2.0; the bundled Noto font
   is OFL-1.1.
4. No scan suppression is accepted silently. A future exception must be a
   reviewed repository change naming the advisory/component, justification,
   owner, expiry date, and replacement/remediation issue. Release notes must
   disclose every active exception. There are currently no exceptions.

The scan database is time-dependent, so a later rebuild can fail even if an
earlier candidate passed. An immutable tag is never overwritten to absorb a
base-image or scan change; increment the version after remediation.

## Publication evidence

Each published digest receives GitHub/Sigstore build-provenance and CycloneDX
SBOM attestations in GHCR. The shared GitHub release also contains:

- `crownline-server-VERSION.cdx.json`, the CycloneDX image SBOM;
- `crownline-server-VERSION-image-scan.json`, the runtime image scan;
- `crownline-server-VERSION-source-scan.json`, the locked dependency/license scan;
- `crownline-server-VERSION-RELEASE.md`, containing the digest, all tags,
  source revision, configuration link, accepted database sources, backup
  requirement, upgrade sequence, and schema-safe rollback warning.

The operational configuration and tested backup/upgrade procedure remain in
[`server-operations.md`](server-operations.md); independent compatibility
boundaries remain in [`compatibility.md`](compatibility.md).
