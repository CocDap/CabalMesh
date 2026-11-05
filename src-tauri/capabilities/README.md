# Capabilities

Two files, one per platform family. There is deliberately **no shared
`default.json`** — see below.

| File | Applies to | Grants |
|---|---|---|
| `desktop.json` | linux, macOS, windows | `core:default`, `opener:default`, the same 38 app commands as mobile.json |
| `mobile.json` | iOS, android | `core:default` plus the same 38 app commands as desktop.json |

Desktop and mobile now serve the same frontend (`src/screens/`) through one
`invoke_handler` in `lib.rs`, so their command grants are kept identical on
purpose — see `tests/handler_arms.rs`, which fails if either file drifts from
the reshaped command list.

## Why the shared capability file was deleted

Capability files **auto-enable** unless the config names identifiers
explicitly, and a window covered by several capabilities receives the
**union** of their permissions.

So adding platform-specific files while leaving a shared one in place scopes
nothing: mobile would still inherit everything the shared file granted,
`opener:default` included, regardless of any `platforms` key on the new files.
The shared file had to go, not be supplemented.

## Where the app command permissions come from

`build.rs` declares every command over IPC in its `COMMANDS` list, and
`tauri-build` generates an `allow-*` and a `deny-*` permission for each.

**A generated permission does nothing until a capability references it.** The
two are only connected by these files, so `COMMANDS` and the `permissions`
arrays here have to move together:

- Add a command to `COMMANDS` but not here → the command exists and is
  unreachable. Calling it fails at runtime, not at compile time.
- Grant a permission here that `COMMANDS` does not declare → the build fails.

Regenerate the list after changing `COMMANDS`:

```sh
python3 - <<'EOF'
import re, json
cmds = re.findall(r'^\s+"([a-z_0-9]+)",\s*$', open("build.rs").read(), re.M)
print(json.dumps(["allow-" + c.replace("_", "-") for c in cmds], indent=2))
EOF
```

## Two things not granted, on purpose

**`core:default` is not minimal.** It bundles `core:app`, `core:event`,
`core:image`, `core:menu`, `core:path`, `core:resources`, `core:tray`,
`core:webview` and `core:window`. Menu and tray are meaningless on a phone.
Narrowing mobile to an enumerated set is worth doing once the command surface
settles — it is listed here so nobody mistakes `core:default` for least
privilege.

**Rust-only plugins get no grant at all.** The `keystore` and
`multicast-lock` plugins are invoked from Rust through `run_mobile_plugin`,
never over IPC. Granting them to the webview would expose vault key
unwrapping to anything that achieves script execution. Only `type-scale` will
ever need a webview grant.

## Never grant speculatively

Both files list exactly the commands the current screens call — nothing more.

An earlier pass granted mobile the full 50-command desktop surface, reasoning
that mobile still served the frozen desktop frontend so it needed them. That
was the wrong instinct: it handed a surface with *no matching screens* the
full command set, including private-key export and raw transaction
submission, purely to keep a placeholder UI from looking broken. Convenience
during development is not a reason to widen an authority boundary.

Desktop carried a similar oversized grant for longer, because it kept the
frozen desktop UI (and its 50-command `legacy` module) working after mobile
moved on. Once that UI was deleted, desktop's grant was cut down to match
mobile's — see git history for the removal.

Add a permission here only when a screen actually calls the command.
