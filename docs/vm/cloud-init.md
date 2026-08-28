# Cloud-Init Guests

`image:` specifies a disk image which is used to provision the guest with
[cloud-init](https://cloud-init.io/).

```
libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  name: freebsd-build
```

## Configuration

`init:` configures early provisioning through conversion into a **cloud-init**
config file.

Repeats of a key are coalesced in the order written, but ordering of e.g.
`packages` with respect to `add` is up to **cloud-init**. See
[Provisioning](./provisioning.md) for late provisioning options.

### `packages:`

Guest packages to install, as an [`Array`](../api/std/array.md) of `Str`.
Names are interpreted by the guest's package manager.

```
libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  init:
    packages:
      - git
      - sqlite3
      - sudo
```

### `add:`

Writes one file during early boot, as a **cloud-init** `write_files` entry.
Content comes from exactly one of `content:` or `source:`.

| Key        | Type                                                   | Description                         |
| ---------- | ------------------------------------------------------ | ----------------------------------- |
| `target`   | [`Str`](../api/std/str.md)                             | Guest path to write                 |
| `source`?  | [artifact spec](./provisioning.md#artifact-specs)      | File to add; excludes `content:`    |
| `content`? | [`Str`](../api/std/str.md)\|[`Bin`](../api/std/bin.md) | Literal content; excludes `source:` |
| `chmod`?   | [`Int`](../api/std/int.md)                             | File mode, e.g. `0o644`             |
| `owner`?   | [`Str`](../api/std/str.md)                             | `user:group` to own the file        |
| `append`?  | [`Bool`](../api/std/bool.md)                           | Append instead of truncating        |
| `defer`?   | [`Bool`](../api/std/bool.md)                           | Write late, after users exist       |

`source:` files or `Bin` `content:` data are base64-encoded, so adding large
files this way is not recommended; a [top-level `add:`](./provisioning.md)
is the place for anything large.

```
libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  init:
    add:
      target: /etc/example.conf
      chmod: 0o644
      content: |
        enabled=yes
    add:
      target: /etc/rc.conf.d/firstboot_pkg_upgrade
      chmod: 0o644
      owner: "root:wheel"
      content: |
        firstboot_pkg_upgrade_enable="NO"
```

### `run:`

A command to run as root during early boot, as a **cloud-init** `runcmd` entry.
A [`Str`](../api/std/str.md) is a shell line and goes in as written, so
redirection, pipelines and expansion work. An
[`Array`](../api/std/array.md) is an argument vector, which the shell does not
get to re-interpret — the way to pass an argument containing whitespace or
shell punctuation.

Unlike the top-level `run:`, this is a command and not a block: Do is not in
the guest yet and there is no VFS to run a block against.

```
libvirt.create
  image: freebsd.qcow2.xz
  os: :FREEBSD:
  init:
    run: mkdir -p /opt/build && echo built > /opt/build/stamp
    run: ["touch", "/opt/build/two words"]
```
