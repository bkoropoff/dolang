# libvirt

The `libvirt` module provisions cloud-init guests and exposes them as
[VFS](./vfs.md) contexts over SSH. It uses an unprivileged
`qemu:///session` connection, passt networking, and a host-loopback SSH port
forward by default.

The host needs `virsh`, `qemu-img`, `passt`, `cloud-localds`, `ssh`, and
`ssh-keygen`. `image:` may be a local path or an HTTP(S) URL; externally
compressed images (`.gz`, `.xz`, etc.) are decompressed into the durable image
store — see [Caching Between Runs](#caching-between-runs) for what that means
for CI.

The domain disk defaults to `20G`. Set `disk_size:` to another `qemu-img`
size when needed. Cloud images with a first-boot growfs service expand their
partition and filesystem into the additional space.

## One-Shot Guest

```
libvirt.with
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  do
    echo $sys.os_info().os
    run uname -a
```

`with` waits for SSH, installs Do into the guest (see [Installing
Do](#installing-do)), waits for boot-time commands and the remote
`dolang-vfs`, enters the VFS for the block, then stops and undefines the
guest even if the block throws.

## Persistent Guest

```
let domain = libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  name: freebsd-build

echo $domain.name
```

A later process can recover the `Domain` object by name.

```
let domain = libvirt.attach freebsd-build
domain.with do
  run uname -a
```

Call `.destroy()` to force-stop a running domain and `.undefine()` to unregister
it. For domains created by this module, `undefine` also removes the ephemeral
overlay after validating the ownership metadata embedded in the domain XML.

Use `upload` and `download` to copy individual files:

```
domain.upload ./input.tar /tmp/input.tar
domain.download /tmp/result.tar ./result.tar
```

To preserve a configured disk, gracefully stop the domain and flatten its
backing chain into a standalone qcow2 image:

```
domain.shutdown()
let image = domain.export_disk ./prepared.qcow2 compress: true
domain.undefine()
```

`shutdown` waits indefinitely for the domain to reach `:SHUTOFF:`. Use
`time.timeout` around it when a time limit is needed. `export_disk` requires
that the domain have exactly one file-backed disk and refuses to overwrite its
destination.

`compress: true` uses qcow2's zstd compression, which needs QEMU 5.1+ to read
the result — the same kind of floor `io:`'s `:IO_URING:` default already sets at
QEMU 6.0+. Pass `compress: :ZLIB:` for an image an older QEMU must open; it is
around nine times slower to write for the same size.

## Gold Images

A disk alone does not describe a guest: the user to log in as, where
`dolang-vfs` lives, which `app` namespace owns the SSH key, and the OS and
architecture are all configuration this module recorded when it created the
domain. `export` writes the disk and that configuration together as a bundle,
and `create bundle:` reconstitutes both.

`build` is the whole minting pass — it provisions a domain exactly as `create`
does, shuts it down cleanly, exports it, and tears it down:

```
libvirt.build ./freebsd-gold.dolvm
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  packages:
    - git
```

```
let domain = libvirt.create
  bundle: ./freebsd-gold.dolvm
  app: dolang-libvirt-test
```

Mint in a pass of its own and always start real work from the bundle, including
on the run that just built it. A caller that provisions when the bundle is
missing and restores when it is present is running its work against two
different guests — one of them freshly cloud-inited, with first-boot services
and a grown filesystem still settling. The divergence lives on the rarer path,
which is where it will be found last.

`Domain.export` is the same operation without the orchestration, for a domain
you already have in hand:

```
domain.shutdown()
domain.export ./freebsd-gold.dolvm
domain.undefine()
```

Restoring runs no provisioning at all — no seed, no cloud-init, no payload
install — so the domain is usable as soon as it accepts SSH. That is the point:
provisioning is what costs minutes on a FreeBSD guest and half an hour on a
Windows one. The `add:` and `run:` actions still run.

Because the bundle already answers them, `os:`, `arch:`, `user:`, `dolang:`,
`packages:`, and `init:` are errors alongside `bundle:` rather than arguments
that quietly do nothing. `app:` must match the one the bundle was exported
under: the guest trusts the SSH key generated for that app, and any other one
leaves it refusing connections. `memory:`, `vcpus:`, and `disk_size:` default
to what the exported domain used, and may be overridden.

A bundle may be a local path or an HTTP(S) URL — it is fetched and cached like
any other download, and the artifact spec's `digest:` pins it.

### The Bundle Format

A bundle is a ZIP holding two members, conventionally named with a `.dolvm`
extension so nothing mistakes it for something `qemu-img` or VirtualBox can
open:

| Member          | Contents                                                                                                                         |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `manifest.json` | Format version, `app`, `os`, `arch`, guest user, VFS command path, memory, vCPUs, and the disk's format, size, and BLAKE3 digest |
| `disk.qcow2`    | The flattened qcow2, compressed by `qemu-img` and `STORE`d as a ZIP member                                                       |

The disk is `STORE`d because `qemu-img convert -c` has already compressed it —
with zstd by default, and in parallel, which takes a ninth of the time zlib does
for the same size. Compressing the qcow2 rather than the ZIP entry keeps the
image compressed in the store it is extracted into, where it backs however many
domains are laid over it; storing it in the ZIP also leaves it a contiguous byte
range that extraction copies out directly. Its BLAKE3 digest is recorded because
a ZIP entry's CRC32 is a corruption check rather than an integrity property, and
the disk is verified again when it is extracted into the image store, where the
bundle's own digest no longer covers it. The manifest also embeds the domain XML
this module generated, as an informational escape hatch; the structured fields
are what reconstitution actually uses.

### Reclaiming Freed Space

A bundle is as large as the disk's *allocated* clusters, not its live data.
Blocks a provisioning run wrote and then deleted are still allocated, still hold
their old contents, and still compress like real data — so a gold image that
installed a toolchain and cleaned up after itself carries the debris.

The domain disk is defined with `discard="unmap"` and `detect_zeroes="unmap"`,
so a guest that trims its filesystem before shutdown actually shrinks the
export, and so does a guest that can only overwrite its free space with zeroes.
Neither reaches qcow2 otherwise: without `discard`, QEMU advertises no discard
feature to the guest and drops the request. Unmapping cannot expose the base
image beneath the overlay — qcow2 records a zero cluster and frees the host
cluster, so the guest still reads zeros.

Issuing the trim is the guest's job, and how depends on it: `fstrim -a` on
Linux, `zpool trim -w` on ZFS, `defrag /L` on Windows. FreeBSD's UFS has no
online batch trim — `tunefs -t enable` only affects later deletions and
`fsck_ffs -E` needs the filesystem unmounted — so there the fallback is to write
zeroes over the free space and delete them.

Neither `export` nor `build` does any of this on its own: what the operation
costs, and whether it can elevate to run it at all, are the caller's to know.
Add it as a `run:` action, which happens before the shutdown `build` performs.

### Clean Shutdown

`export` refuses a domain that did not reach `:SHUTOFF:` through a guest
shutdown — one that was destroyed or that crashed has a disk in an arbitrary
state, spectacularly so under `cache: :UNSAFE:`, where guest flushes are
discarded outright. libvirt reports the reason as `unknown` once it has
restarted, so `force: true` exists for a domain that did shut down cleanly but
can no longer prove it.

## Running a Script in a Domain

The bundled `libvirt` entrypoint compiles a local script and runs it through an
existing Do-created domain's VFS:

```
dolang -m libvirt freebsd-build build.dol release
```

Every argument after the script path is passed through unchanged. Inside the
target script, `shell.program` is the local script path and `shell.args`
contains only those trailing arguments.

`--cd` sets the initial remote working directory; repeat `--env` to set or
inherit (a bare `NAME`, with no `=`, inherits the local value) an
environment variable; `--unset-env` unsets one:

```
dolang -m libvirt --cd build --env CARGO_TERM_COLOR freebsd-build build.dol
```

Use `--connect-uri` when it is registered with a connection other than
`qemu:///session`:

```
dolang -m libvirt --connect-uri qemu:///system freebsd-build build.dol
```

## Target Platform

`os:` is required, and `arch:` defaults to the host architecture. Both use the
same symbol vocabulary as
[`sys.os_info().os`](../api/sys/index.md)/[`sys.cpu_info().arch`](../api/sys/index.md).

The domain is defined for KVM with a `host-passthrough` CPU, so `arch:` must be
the host architecture; a foreign one is rejected rather than emulated.

## Artifact Specs

Everything this module fetches — `image:`, `bundle:`, `dolang:`, and an `add:`
`source:` — is spelled the same two ways. Either a bare path or URL:

```
libvirt.create
  image: https://example.com/disk.qcow2
  os: :LINUX:
```

or a block carrying the source as its one dash item, with metadata alongside
it:

```
libvirt.create
  image:
    - https://example.com/disk.qcow2
    digest: blake3:0123...
  os: :LINUX:
```

| Key       | Type                                                                                  | Description                              |
| --------- | ------------------------------------------------------------------------------------- | ---------------------------------------- |
| `0`       | [`Str`](../api/std/str.md)\|[`Path`](../api/fs/path.md)\|[`Url`](../api/url/index.md) | The source, as the block's one dash item |
| `digest`? | [`Str`](../api/std/str.md)                                                            | `algorithm:hex` digest to verify against |

A local path is verified in place. A URL is downloaded, verified, and cached,
and a digest-pinned entry that has already been verified is reused without
contacting the server. This one shape is why there is no `image_digest:`
beside `image:` and no `bundle_digest:` beside `bundle:`.

## Installing Do

`dolang:` controls whether and how Do (particularly `dolang-vfs`) gets
installed into the guest before it's considered ready:

| Value                          | Behavior                                                                                                                                                                                    |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `false`                        | Skip installation. The domain won't support `with`, `upload`, or `download`.                                                                                                                |
| `true` (default)               | Best-effort: fetch the release matching the running interpreter's version ([`shell.VERSION`](../api/shell/index.md)) for `os:`/`arch:`, silently skipping if none exists for that platform. |
| a version tag, e.g. `"v0.1.1"` | Fetch that release's artifact for `os:`/`arch:`. Throws an error if unavailable.                                                                                                            |
| a [`Path`](../api/fs/path.md)  | Use a local archive directly.                                                                                                                                                               |
| a [`Url`](../api/url/index.md) | Fetch that archive directly, bypassing release resolution.                                                                                                                                  |
| an artifact spec               | An archive with a pinned digest — see [Artifact Specs](#artifact-specs).                                                                                                                    |
| `{version: tag}`               | Explicit form of the version tag.                                                                                                                                                           |

Except for the implicit `true` default, a failed fetch always throws an error.

## Provisioning

`packages:` installs explicit guest packages. `add:` adds a file to the
filesystem, while `run:` runs a block within the VM VFS before creation is
considered complete; these may be repeated.

`init:` configures boot-time (cloud-init) provisioning, supporting `add:` and
`run:` keys; however, `run:` only supports raw commands as strings or string
arrays.

Both forms of `add:` take `chmod:` as an [`Int`](../api/std/int.md) mode
such as `0o644`.

```
let vm = libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  packages:
    - git
  init:
    add:
      target: /etc/example.conf
      chmod: 0o644
      content: |
        enabled=yes
    run: mkdir -p /opt/build
  run: do
    cd /opt/build do
      git clone https://github.com/dolang-org/dolang
```

## Caching Between Runs

Everything this module persists lives under two directories, scoped by
`app:` (default: derived from `shell.program`).

`$XDG_DATA_HOME/<app>/libvirt/`:

| Path       | Contents                 |
| ---------- | ------------------------ |
| `ssh/`     | SSH keys                 |
| `seeds/`   | Cloud-init seed ISOs     |
| `images/`  | Base disk images         |
| `domains/` | Per-domain working state |

`$XDG_CACHE_HOME/<app>/`:

| Path        | Contents       |
| ----------- | -------------- |
| `transfer/` | Download cache |

Of these, it is recommended that you retain `ssh/` and `seeds/` in
`$XDG_DATA_HOME` and the entire cache directory in `$XDG_CACHE_HOME`. SSH
public keys are baked into seed ISOs, so losing the private keys will
necessitate regenerating them.

Pass an explicit `app:` if you need to use the same cache from multiple
scripts, as the default is derived from `shell.program`.
