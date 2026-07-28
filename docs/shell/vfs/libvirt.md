# libvirt

The `libvirt` module provisions cloud-init guests and exposes them as
[VFS](./index.md) contexts over SSH. It uses an unprivileged
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
[`sys.os_info().os`](../../api/sys/index.md)/[`sys.cpu_info().arch`](../../api/sys/index.md).

## Installing Do

`dolang:` controls whether and how Do (particularly `dolang-vfs`) gets
installed into the guest before it's considered ready:

| Value                              | Behavior                                                                                                                                                                                       |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `false`                            | Skip installation. The domain won't support `with`, `upload`, or `download`.                                                                                                                   |
| `true` (default)                   | Best-effort: fetch the release matching the running interpreter's version ([`shell.VERSION`](../../api/shell/index.md)) for `os:`/`arch:`, silently skipping if none exists for that platform. |
| a version tag, e.g. `"v0.1.1"`     | Fetch that release's artifact for `os:`/`arch:`. Throws an error if unavailable.                                                                                                               |
| a [`Path`](../../api/fs/path.md)   | Use a local archive directly.                                                                                                                                                                  |
| a [`Url`](../../api/url/index.md)  | Fetch that archive directly, bypassing release resolution.                                                                                                                                     |
| `{archive: path_or_url, :digest?}` | Explicit form that allows pinning a digest.                                                                                                                                                    |

Except for the implicit `true` default, a failed fetch always throws an error.

## Provisioning

`packages:` installs explicit guest packages. `add:` adds a file to the
filesystem, while `run:` runs a block within the VM VFS before creation is
considered complete; these may be repeated.

`init:` configures boot-time (cloud-init) provisioning, supporting `add:` and
`run:` keys; however, `run:` only supports raw commands as strings or string
arrays.

```
let vm = libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  packages:
    - git
  init:
    add:
      target: /etc/example.conf
      chmod: "0644"
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
