# TokenGroupAttributes

Windows access-token group attributes.

## Constructor

### `TokenGroupAttributes ...attributes`

Constructs flags from attribute symbols.

| Symbol                 | Attribute                              |
| ---------------------- | -------------------------------------- |
| `:MANDATORY:`          | Mandatory group                        |
| `:ENABLED_BY_DEFAULT:` | Enabled by default                     |
| `:ENABLED:`            | Enabled                                |
| `:OWNER:`              | Eligible as an object owner            |
| `:USE_FOR_DENY_ONLY:`  | Used only for deny checks              |
| `:INTEGRITY:`          | Integrity SID                          |
| `:INTEGRITY_ENABLED:`  | Integrity SID enabled                  |
| `:RESOURCE:`           | Resource group                         |
| `:LOGON_ID:`           | Logon-session identifier               |

## Methods

### `contains attribute`

Tests whether an attribute is set.

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields selected symbols.
