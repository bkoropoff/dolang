# transfer

Downloads, uploads, packs, and safely extracts artifacts.

`get` accepts a local path or an HTTP(S) URL. URL responses are cached under
the application cache directory and revalidated with HTTP validators when
available. Pass `digest:` as `algorithm:hex` to verify an artifact; a cache
entry already verified against that digest can be reused without a request.

`pack` supports TAR, gzip-compressed TAR, Zstandard-compressed TAR, and ZIP.
`unpack` additionally decompresses raw gzip, Zstandard, and XZ streams into a
file. It distinguishes compressed TAR archives from raw streams by filename:
for example, `.tar.gz` selects TAR while `.gz` selects a raw stream. Raw
decompression requires the corresponding `gzip`, `zstd`, or `xz` program.

Archive extraction first validates the complete manifest, rejects unsafe or
conflicting paths, and writes into a staging directory before publishing the
destination. Raw decompression also stages its output before publication.

---

::: transfer
