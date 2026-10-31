# Supplemental third-party notices

These files keep their upstream licenses. They are not relicensed under the
Metric Owl application license.

`manifest.json` records the package versions, original source URLs, upstream
commits where available, and SHA-256 hashes. Most supplements restore license
files omitted from a published Cargo archive. The QR supplement is the original
Project Nayuki MIT notice bundled in `qrcode.react` 4.2.0. The `r-efi` AUTHORS
files include their original license terms and copyright holders.

The musl 1.2.5 COPYRIGHT file comes from the official versioned source archive.
The Linux preview.8 link map selected the Rust target's self-contained `libc.a`,
whose version is 1.2.5. The build host's separately installed musl package is not
used as a substitute for that version. Review this record when the Rust target
or linker configuration changes. The Linux release also includes its installed
Rust toolchain's library copyright/license notices.

`scripts/collect-licenses.py` verifies these hashes and copies the applicable
notices alongside all locked Cargo packages and production JavaScript
dependencies, including declared Cargo `license_file` paths. A package in that
scope without collected notice text causes a failed collection and an explicit
report in `artifacts/release/license-collection-report.json`.

The complete development JavaScript metadata remains in
`javascript-licenses.json`; the shipped dependency inventory is separately named
`javascript-runtime-licenses.json`. Development tools are not bundled into the
application. `development-notice-gaps.json` explicitly records that Vitest's
development-only `stackback` 0.0.2 declares MIT but its published archive and
upstream release omit a full license notice file. No copyright attribution was
invented to fill that gap. These inventories are not a certification that every
legal obligation has been independently reviewed.
