# base64

Base64 encoding and decoding (RFC 4648).

## Alphabets

| Symbol       | Description                                               |
| ------------ | --------------------------------------------------------- |
| `:STANDARD:` | Standard alphabet, using `+` and `/` (RFC 4648 section 4) |
| `:URL:`      | URL-safe alphabet, using `-` and `_` (section 5)          |
| `:AUTO:`     | Decoding only; detect the alphabet from the input         |

## Functions

### `decode text :alphabet? :pad?`

Decodes base64 text and returns the raw bytes.

By default both alphabets and any amount of padding are accepted. Specifying
`alphabet:` or `pad:` makes decoding strict in that respect.

#### Parameters

| Name       | Type                                         | Description                    |
| ---------- | -------------------------------------------- | ------------------------------ |
| `text`     | [`Str`](./std/str.md)\|[`Bin`](./std/bin.md) | base64 text to decode          |
| `alphabet` | [`Sym`](./std/sym.md)?                       | Alphabet; default `:AUTO:`     |
| `pad`      | [`Bool`](./std/bool.md)?                     | Required padding; default none |

##### Alphabet Detection

`:AUTO:` selects the URL-safe alphabet if the input contains `-` or `_`, and
the standard alphabet otherwise. Input containing characters from both
alphabets is rejected. Input containing neither decodes identically under
both.

##### Padding

When `pad:` is omitted, canonical padding and any lesser amount — including
none — are accepted. `pad: true` requires canonical padding; `pad: false`
requires that padding be absent.

#### Returns

[`Bin`](./std/bin.md) - Decoded bytes

#### Errors

| Exception    | Condition                                                               |
| ------------ | ----------------------------------------------------------------------- |
| `TypeError`  | `text` is not a string or binary, or an option has the wrong type       |
| `ValueError` | The input is not valid base64, or `alphabet` is not a recognized symbol |

#### Example

```
assert_eq (decode "aGVsbG8=") b"hello"
assert_eq (decode "aGVsbG8") b"hello"
assert_eq (decode $ encode "hello") b"hello"
assert_eq (decode "-_8" alphabet: :URL: pad: false) b"\xfb\xff"
```

### `encode data :alphabet? :pad?`

Encodes a string or binary value as base64 text.

#### Parameters

| Name       | Type                                         | Description                     |
| ---------- | -------------------------------------------- | ------------------------------- |
| `data`     | [`Str`](./std/str.md)\|[`Bin`](./std/bin.md) | data to encode                  |
| `alphabet` | [`Sym`](./std/sym.md)?                       | Alphabet; default `:STANDARD:`  |
| `pad`      | [`Bool`](./std/bool.md)?                     | Emit padding; default see below |

##### Padding

`pad:` defaults to `true` for `:STANDARD:` and `false` for `:URL:`, since
URL-safe base64 is conventionally unpadded (for example in JSON Web
Signatures, RFC 7515). Pass `pad:` explicitly to override.

#### Returns

[`Str`](./std/str.md) - Base64 text

#### Errors

| Exception    | Condition                                                         |
| ------------ | ----------------------------------------------------------------- |
| `TypeError`  | `data` is not a string or binary, or an option has the wrong type |
| `ValueError` | `alphabet` is not a recognized symbol                             |

#### Example

```
assert_eq (encode "") ""
assert_eq (encode "hello") "aGVsbG8="
assert_eq (encode b"hello") "aGVsbG8="
assert_eq (encode b"\xfb\xff") "+/8="
assert_eq (encode b"\xfb\xff" alphabet: :URL:) "-_8"
assert_eq (encode "hello" pad: false) "aGVsbG8"
```
