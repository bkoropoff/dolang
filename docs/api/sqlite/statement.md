# Statement

Statement objects are returned by
[`Connection.prepare()`](./connection.md#prepare-sql-func) and represent
compiled SQL statements that can be executed multiple times with different
parameters.

## Methods

### `close()`

Closes the statement and releases resources.

### `execute args...`

Executes the statement and returns the number of rows affected.

#### Parameters

| Name  | Type | Description                                           |
| ----- | ---- | ----------------------------------------------------- |
| `...` | any  | Values for the statement's parameters                 |

#### Returns

`Int` - Number of rows affected

#### Errors

| Exception                                                | Condition                     |
| -------------------------------------------------------- | ----------------------------- |
| [`MissingPosError`](../std/missing-pos-error.md)         | A numbered parameter unfilled |
| [`MissingKeyError`](../std/missing-key-error.md)         | A named parameter unfilled    |
| [`UnexpectedPosError`](../std/unexpected-pos-error.md)   | A positional argument unused  |
| [`UnexpectedKeyError`](../std/unexpected-key-error.md)   | A keyword argument unused     |

#### Example

```
open "mydb.sqlite" do |conn|
  conn.prepare t"UPDATE users SET status = ${#status} WHERE created < ${#date}"
    do |stmt|
      let affected = stmt.execute status: "archived" date: "2023-01-01"
      echo "Archived $(affected) users"
```

### `query args...`

Executes the statement and returns a Rows iterator for reading results.

#### Parameters

| Name  | Type | Description                                           |
| ----- | ---- | ----------------------------------------------------- |
| `...` | any  | Values for the statement's parameters                 |

#### Returns

[Rows](./rows.md)

#### Errors

The same as [`execute`](#execute-args).

#### Example

```
open "mydb.sqlite" do |conn|
  conn.prepare t"SELECT * FROM users WHERE age > ${#min_age}" do |stmt|
    for row = stmt.query min_age: 18
      echo "$(row["name"]) is $(row["age"]) years old"

    let count = 0
    for row = stmt.query min_age: 21
      count += 1
    echo "Found $(count) adults"
```

## Usage Notes

### Parameter Binding

A statement is prepared from a [template](../std/fmt.md), and a template says
two different things about the values in it.

An **interpolation** — `$name` or `${...}` — carries a value the program already
has. It is bound when the statement is prepared and stays bound for the life of
the statement.

A **parameter** — `${#name}` or `${#0}` — is a hole, left for each call to fill.
A named parameter is filled by a keyword argument and a numbered one by the
positional argument in that place; a hole used twice is one parameter, filled
once.

```
let cutoff = "2023-01-01"
conn.prepare t"UPDATE users SET status = ${#status} WHERE created < $cutoff"
  do |stmt|
    stmt.execute status: "archived"
```

Filling is exhaustive: every parameter must be supplied on every call, and an
argument naming no parameter raises. Use
[`Fmt.bind`](../std/fmt.md#bind-bindings) to fill some holes before preparing,
and [`Fmt.params`](../std/fmt.md#params) to ask what a template still wants.

Neither form ever becomes SQL text — only the template's literal text does — so
an interpolated value cannot alter the statement it appears in, whatever it
contains.

#### Rejected at prepare

| Condition                                   | Why                                                 |
| ------------------------------------------- | --------------------------------------------------- |
| The SQL is a [`Str`](../std/str.md)         | A `Str` cannot say which of its text is data        |
| A `:name` or `?` in the template's own text | The binder cannot see it, so it would step as NULL  |
| A quoted interpolation, as in `'$name'`     | Quoting buries the value in a literal, binding none |
| A specification, as in `${#0:>10}`          | A bound value is never rendered, so it takes none   |

The quoting case is worth naming, because it is the habit a plain string
teaches: `t"... WHERE name = '$name'"` needs no quotes, since `$name` is bound
rather than pasted. Written with them, it raises rather than doing the wrong
thing quietly.

#### Supported parameter types

| Type    | SQLite Type  | Example                                  |
| ------- | ------------ | ---------------------------------------- |
| `nil`   | NULL         | `stmt.execute value: nil`                |
| `Bool`  | INTEGER      | `stmt.execute active: true`              |
| `Int`   | INTEGER      | `stmt.execute id: 42`                    |
| `Float` | REAL         | `stmt.execute price: 19.99`              |
| `Str`   | TEXT         | `stmt.execute name: "Alice"`             |
| `Bin`   | BLOB         | `stmt.execute data: b"\x01\x02\x03"`     |

`Bool` values are stored as `0` or `1`. The same types are accepted for an
interpolated value.

#### Example

```
open "mydb.sqlite" do |conn|
  conn.prepare
    t"INSERT INTO users (name, age, active) VALUES (${#name}, ${#age}, ${#active})"
    do |stmt|
      stmt.execute name: "Alice" age: 30 active: true
      stmt.execute name: "Bob" age: 25 active: false
```

### Automatic Retry

When a statement is executed outside of a transaction, busy errors are
automatically retried according to the connection's retry configuration.

### Concurrent Use

Only one query can be active on a statement at a time. Starting a new query or
executing another statement invalidates the previous query iterator, which will
subsequently raise a concurrency error on use.

```
open "mydb.sqlite" do |conn|
  conn.prepare t"SELECT * FROM users" do |stmt|
    let rows = stmt.query()
    let rows2 = stmt.query()
    # rows has been invalidated at this point
```
