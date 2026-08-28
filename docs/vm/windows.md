# Windows Guests

Windows guests are initially provisioned from install media named by
`installer:` and `drivers:` arguments:

```
libvirt.with
  installer: Win11_24H2_x64.iso
  drivers: virtio-win.iso
  os: :WINDOWS:
  do
    run cmd /c ver
```

Early provisioning stops in audit mode, which is the point at which
[actions](./provisioning.md#actions) are run and `create` returns the
guest.

Further provisioning and transitioning out of audit mode with `sysprep` are up
to the user. You probably want to create a gold image to avoid the install
process when creating future guests; see [`Gold Images`](./gold-images.md).

Windows guests are created with a VNC display and keyboard/mouse inputs in case
manual intervention via GUI is necessary. Defaults differ as well: `memory:` is
4096 MiB, `disk_size:` is `64G` — the Windows 11 minimum — and `wait_timeout:`
is an hour rather than fifteen minutes, as an install takes considerable time.
All may be overridden as for any other guest.

## Host Requirements

`libvirt` must be **10.10.0 or newer**. `swtpm` and `swtpm_setup` are necessary
for the virtual Trusted Platform Module, and either `xorrisofs` or `mkisofs` to
author the provisioning disc.

## Media

In addition to install media,
[`virtio` drivers](https://github.com/virtio-win/virtio-win-pkg-scripts/blob/master/README.md)
must be provided via `drivers:`, or Windows Setup will not be able to access
virtual disks, NICs, etc.

Both `installer:` and `drivers:` may be repeated. The first `installer:` is what
the guest boots; additional instances are attached in order, for media split
across more than one image. `drivers:` are read by Setup but not booted.

Media carrying more than one edition must include `edition:` to specify which to
install, or alternatively `index:` to specify one by position. If not specified,
the unattended install will hang.

```
libvirt.create
  installer:
    source: Win11_24H2_x64.iso
    edition: Windows 11 Pro
  drivers: virtio-win.iso
  os: :WINDOWS:
```

## Troubleshooting

The provisioning script's own transcript is at `C:\dolang\provision.log`, and
Setup's logs are under `%WINDIR%\Panther`.

A timed out install takes a screenshot of the console in
`$XDG_DATA_HOME/<app>/libvirt/screenshots/`.
