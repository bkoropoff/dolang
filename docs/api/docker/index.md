# docker

Runs and manages Docker containers and images.

## Create and Run Parameters

`create` and `run` share these parameters. `cmd` is required by `run` and
optional for `create`.

| Name         | Type                             | Description                                 |
| ------------ | -------------------------------- | ------------------------------------------- |
| `image`      | [`Str`](../std/str.md)           | Image name or ID                            |
| `cmd`        | [`Str`](../std/str.md)           | Command to run instead of the image command |
| `cd`         | [`Str`](../std/str.md)           | Optional container working directory        |
| `env`        | [`Dict`](../std/dict.md)         | Container environment                       |
| `pull`       | [`Sym`](../std/sym.md)           | Image pull policy; default `:MISSING:`      |
| `name`       | [`Str`](../std/str.md)           | Optional container name                     |
| `mounts`     | [`Iterable`](../std/iterable.md) | Mount specifications                        |
| `labels`     | [`Dict`](../std/dict.md)         | Container labels                            |
| `ports`      | [`Iterable`](../std/iterable.md) | Published-port specifications               |
| `networks`   | [`Iterable`](../std/iterable.md) | Network names or IDs                        |
| `user`       | [`Str`](../std/str.md)           | Optional user or `user:group`               |
| `entrypoint` | [`Str`](../std/str.md)           | Optional entrypoint override                |
| `args`       | [`Value`](../std/value.md)       | Arguments passed to `cmd`                   |

### `pull`

| Value       | Meaning                                  |
| ----------- | ---------------------------------------- |
| `:MISSING:` | Pull the image only when it is not local |
| `:ALWAYS:`  | Always pull the image                    |
| `:NEVER:`   | Never pull the image                     |

### `env`

Keys are strings or symbols. A value of `:INHERIT:` copies the variable from
the host environment; other values are converted to strings. `nil` values are
not supported.

### `mounts`

Each element is a [`Dict`](../std/dict.md) with these keys:

| Name       | Type                        | Meaning                                      |
| ---------- | --------------------------- | -------------------------------------------- |
| `type`     | [`Sym`](../std/sym.md)      | `:BIND:`, `:VOLUME:`, or `:TMPFS:`           |
| `target`   | [`Value`](../std/value.md)  | Mount path in the container                  |
| `source`   | [`Value`](../std/value.md)  | Host path or volume name; required for binds |
| `readonly` | [`Bool`](../std/bool.md)    | Mount read-only when `true`; default `false` |

### `ports`

Each element is a [`Dict`](../std/dict.md) with these keys:

| Name             | Type                        | Meaning                                      |
| ---------------- | --------------------------- | -------------------------------------------- |
| `container_port` | [`Value`](../std/value.md)  | Container port                               |
| `host_port`      | [`Value`](../std/value.md)  | Host port; assigned automatically if omitted |
| `host_ip`        | [`Str`](../std/str.md)      | Optional host address to bind                |
| `protocol`       | [`Sym`](../std/sym.md)      | `:TCP:` (default), `:UDP:`, or `:SCTP:`      |

::: docker
