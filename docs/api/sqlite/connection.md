# Connection

Connection objects are returned by
[`open()`](./index.md#open-path-retries-min_wait-max_wait-func)
and provide methods for interacting with SQLite databases.

## Methods

### `close()`

Closes the database connection and releases resources. Connections not
explicitly closed are closed when garbage collected.

### `execute sql args...`

Prepares a SQL statement, executes it, and returns the number of rows affected.
This is a convenience shorthand for [`prepare`](#prepare-sql-func) +
[`Statement.execute`](./statement.md#execute-args) when a statement doesn't
need to be reused.

#### Parameters

| Name  | Type                   | Description                                  |
| ----- | ---------------------- | -------------------------------------------- |
| `sql` | [`Fmt`](../std/fmt.md) | SQL template                                 |
| `...` | any                    | Values for the template's parameters         |

##### SQL Is a Template

`sql` is a [formatted sequence](../std/fmt.md) — a `t"..."` — and a
[`Str`](../std/str.md) raises. Only the literal text of the template becomes
SQL; an interpolated value is bound as a parameter, so it is data whatever it
looks like. See [Parameter Binding](./statement.md#parameter-binding).

#### Returns

`Int` — number of rows affected

#### Example

```
open "mydb.sqlite" do |conn|
  conn.execute t"CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"

  let name = "Alice"
  conn.execute t"INSERT INTO users (name) VALUES ($name)"
```

### `prepare sql func?`

Prepares a SQL statement for repeated execution.

#### Parameters

| Name   | Type                   | Description                                               |
| ------ | ---------------------- | --------------------------------------------------------- |
| `sql`  | [`Fmt`](../std/fmt.md) | SQL template                                              |
| `func` | `Func`                 | Function to run with the statement; auto-closes when done |

A parameter written `${#name}` or `${#0}` stays unfilled and is supplied at each
[`query`](./statement.md#query-args) or
[`execute`](./statement.md#execute-args). An interpolated value is bound once,
when the statement is prepared.

#### Returns

[Statement](./statement.md) when no `func` is provided, otherwise
the result of calling `func`

#### Example

```
open "mydb.sqlite" do |conn|
  # Using block form (auto-closes)
  conn.prepare t"SELECT * FROM users WHERE id = ${#id}" do |stmt|
    for row = stmt.query id: 1
      echo "User: $(row["name"])"

  # Manual management
  let stmt = conn.prepare t"INSERT INTO users (name) VALUES (${#name})"
  stmt.execute name: "Charlie"
  stmt.close()
```

### `transaction func`

Begins a database transaction and passes a [Transaction](./transaction.md)
object to the provided block.

#### Parameters

| Name   | Type   | Description                            |
| ------ | ------ | -------------------------------------- |
| `func` | `Func` | Function to run within the transaction |

#### Returns

The result of calling `func`

The transaction is automatically committed when `func` returns successfully and
automatically rolled back if it raises an error. Call
[`commit()`](./transaction.md#commit) or
[`rollback()`](./transaction.md#rollback) explicitly to finalize the transaction
early.

When a busy error occurs inside a transaction, the operation raises immediately
without retrying. The transaction block is then rolled back and re-invoked
until it succeeds, is explicitly rolled back, or retries are exhausted.

#### Example

```
open "mydb.sqlite" do |conn|
  conn.transaction do |_|
    conn.execute t"UPDATE accounts SET balance = balance - 100 WHERE id = 1"
    conn.execute t"UPDATE accounts SET balance = balance + 100 WHERE id = 2"

  # Explicit rollback example
  conn.transaction do |tx|
    conn.execute t"INSERT INTO audit (action) VALUES ('attempt')"
    if should_cancel
      tx.rollback()
```

## Usage Notes

### Busy Retry

When an operation encounters a busy error outside of a transaction, it is
automatically retried with exponential backoff. The retry parameters are
configured when opening the connection:

| Parameter  | Default | Description                             |
| ---------- | ------- | --------------------------------------- |
| `retries`  | 10      | Maximum number of retry attempts        |
| `min_wait` | 1       | Initial wait in milliseconds            |
| `max_wait` | 1000    | Maximum wait in milliseconds (cap)      |

The wait time doubles after each attempt (plus a small random jitter) until it
reaches `max_wait`. Set `retries: 0` to disable automatic retry.

Operations within a transaction are not retried individually; instead the
entire transaction is retried. See [Transaction](./transaction.md) for details.

```
# High-contention scenario: retry up to 20 times, wait up to 5s
open "mydb.sqlite" retries: 20 max_wait: 5000 do |conn|
  conn.execute t"UPDATE counters SET value = value + 1"

# Disable automatic retry
open "mydb.sqlite" retries: 0 do |conn|
  do
    conn.execute t"UPDATE counters SET value = value + 1"
  catch Busy
    echo "Database is busy"
```

### Concurrency

A connection may only be used by one strand at a time. Concurrent access from
multiple strands raises a concurrency error.
