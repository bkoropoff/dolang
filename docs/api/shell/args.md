# Args

Immutable command-line argument sequence returned by
[`shell.args`](./index.md#args).

## Fields

### `len`

Number of arguments.

```
echo $shell.args.len
```

## Operators

`Args` supports integer and range indexing, iteration, spreading, and
positional destructuring. Indexed assignment raises
[`ImmutableError`](../std/immutable-error.md).

```
if shell.args
  echo "first: $(shell.args[0])"

let first ...rest = shell.args
run tool ...rest
```
