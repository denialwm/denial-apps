# denial-plugins

Denial's plugin manager: it keeps the user's plugin selection, builds the
shell with those plugins and asks deniald to run it. Denial's
`docs/PLUGIN_MANAGER.md` is the contract, sections 21 to 25 the current
part.

Settings links this library and runs requests on its own threads. The
`denial-plugins` command (`src/bin/denial-plugins.rs`) adds the command
line, `submit` for programs that need a request to outlive them, and
`resume`, which deniald starts after a Denial update. Both read
`denial-plugins.installation.json` beside the command's real path.

| Module | What |
|---|---|
| `manager` | The commands, requests and their job records |
| `store` | The state folder, `$XDG_STATE_HOME/denial/plugins`, and its lock |
| `source`, `available` | Plugins from Git, folders and Denial itself; the community list |
| `graph`, `boundary`, `preflight` | The package graph, the SDK boundary, what blocks a selection |
| `discovery` | Runs Denial's `packages/denial_plugin_discovery/discover.dart` |
| `generator`, `workspace` | Provider choice, the generated shell, the plan |
| `tools`, `builder` | The Flutter tool and compilers; the build |
| `activation`, `rebuild` | deniald's side: switching, undo, off, rebuilding after an update |
| `resume`, `notifications` | The rebuild notification after an update |
| `report`, `progress` | What `status`, `list` and `history` say, and the step labels |

Plugin discovery stays Dart: the script runs with the Dart and the analyzer
of the installation's Flutter tool, and its kernel snapshot is cached in the
state folder at first use. Flutter's build tool and Pub stay programs of
their own.

It is a member of Settings' Cargo workspace (`apps/settings`), with its lock.
`cargo test -p denial-plugins` there runs its tests.
