# scooper

```raw
 ___                                                 .-.
| _ ) __ _ __ _ __ _ ___ _ _                        /   \
| _ \/ _` / _` / _` / -_) '_|        _____.....-----|(o) |
|___/\__,_\__, \__, \___|_|    _..--'          _..--|  .''
          |___/|___/         .'  o      _..--''     |  | |
                            /  _/_..--''            |  | |
                   ________/  / /                   |  | |
                  | _  ____\ / /                    |  | |
 _.-----._________|| ||    \\ /                     |  | |
|=================||=||_____\\                      |__|-'
|                 ||_||_____//                      (o\ |
|_________________|_________/                        |-\|
 `-------------._______.----'                        /  `.
    .,.,.,.,.,.,.,.,.,.,.,.,.,                      /     \
   ((O) o o o o ======= o o(O))                 ._.'      /
    `-.,.,.,.,.,.,.,.,.,.,.,-'                   `.......'

          scooper 0.1.0 - same buckets, heavy machinery
```

Scooper is a Rust rewrite of Scoop for Windows. While Scoop feels like a flimsy spoon, scooper is 100 ton highly efficient machine. One native
binary, no PowerShell startup, working with the Scoop buckets you already use.

Moves mountains. Installs apps.

Scooper is the DarbotLM package-management and repository-understanding
frontend. Alongside the Scoop-compatible CLI, it exposes a semantic object
model and spatial knowledge graph for tools and agents.

[![crate](https://img.shields.io/crates/v/scooper)](https://crates.io/crates/scooper)
[![license][license-badge]](LICENSE)

![scooper upgrade demo](demo/scooper-upgrade-cut.gif)

## Why an excavator to install `jq`

- **Speed.** Native binary, no shell startup cost, parallel manifest parsing.
  Numbers below, measured — not projected.
- **Zero setup.** One 8.5 MB `.exe`. No execution-policy fiddling: the
  installer runs in-memory (like Scoop's own), and scooper passes
  `-ExecutionPolicy Bypass` itself whenever a manifest needs PowerShell.
- **Compatible.** Reads your existing Scoop buckets, `config.json`, cache,
  and `apps/` layout in place. Set `SCOOP` to your current root and keep
  digging. See the [compatibility table](#compatibility) for exactly
  what that promise covers — and what it doesn't.

## Benchmarks

Median of 5 runs, fresh process each time, one warmup discarded.
Ryzen 7 3700X, Windows, 102 installed apps, 7 buckets.
`scooper` 1.0.0 (release build) vs Scoop at current `master`
invoked through its `scoop.ps1` shim (which is the point: the ~1 s
PowerShell startup is included in Scoop's column).

| Operation            | Scoop (PS) | scooper | Speedup |
| :---                 | ---:       | ---:   | ---:    |
| `--version`          | 993 ms     | 14 ms  | ~71x    |
| `list`               | 1405 ms    | 26 ms  | ~54x    |
| `search python`      | 1515 ms    | 30 ms  | ~51x    |
| `info 7zip`          | 810 ms     | 26 ms  | ~31x    |

The manifest parser is parallel, so parsing 6,100 manifests takes ~30 ms
*without* any cache — enabling `use_sqlite_cache` measured 31 ms on the
same query. The cache exists for constrained disks, not for speed.

## Compatibility

Scooper shares Scoop's on-disk surfaces, so the two tools can work the same
root. Anything not listed here is not promised.

| Surface | Scoop | scooper | Notes |
| :--- | :---: | :---: | :--- |
| Bucket dirs + manifest JSON | ✓ | ✓ | reads your existing buckets in place; all 7 built-in buckets known |
| `config.json` keys | ✓ | ✓ | same file (`config list` prints its path); unknown keys ignored |
| `apps/<app>/<version>` + `current` junction | ✓ | ✓ | identical layout; upgrades keep version history |
| `install.json` (arch/bucket/url) | ✓ | ✓ | written on every install |
| Cache names `app#version#sha7.ext` | ✓ | ✓ | same naming scheme — caches are interchangeable |
| `shims/` (`.exe`/`.cmd`/`.ps1`/`.shim`) | ✓ | ✓ | same directory, symmetric add/remove |
| `persist/`, `modules/`, shortcuts | ✓ | ✓ | same locations |
| `-g/--global` under `%ProgramData%\scoop` | ✓ (admin) | ✓ (admin-gated) | gate verified; cache stays user-scoped |
| Hook scripts (`$dir`, `Expand-7zipArchive`, …) | ✓ | ✓ (shipped in `0.1.0-beta.10`) | full hook scope + archive helpers |
| `SCOOPER_FLAVOR=heavy` progress strings | — | ✓ | opt-in only; default output is script-compatible |
| Self-update (`update scoop`) | ✓ | — | reinstall via `install.ps1` or `cargo install -f scooper` |
| Non-Windows | — | — | Windows-only, like Scoop |

## Install

### One-liner (Windows, user scope)

```ps1
iwr -useb https://raw.githubusercontent.com/DarbotLM/scooper/main/scripts/install.ps1 | iex
```

The installer also wires up PowerShell tab-completion (PowerShell 7+;
Windows PowerShell 5.1 ignores it silently) — flags, subcommands, and
live app/bucket/config names (`uninstall neo` completes to `neovim`).
Already installed? Run `scooper completions powershell --install`
(or `-NoCompletions` to skip).

System-wide instead (requires admin):

```ps1
iex (iwr -useb https://raw.githubusercontent.com/DarbotLM/scooper/main/scripts/install.ps1).Content -args -System
```

### From Scoop

```ps1
scoop bucket add scooper https://github.com/DarbotLM/scooper-bucket
scoop install scooper
```

### From crates.io

```sh
cargo install scooper
```

### Migrating from Scoop

No migration needed to try it: scooper uses the same `~\scoop` root by
default and reads everything Scoop installed — `list`, `status`,
upgrades, and uninstalls all operate on the existing apps in place.

For a side-by-side trial (or a fresh root), import an export file;
missing buckets are added automatically and holds are restored:

```ps1
scoop export > scoopfile.json
$env:SCOOP = "$HOME\scoop-scooper"  # omit to use the shared root
scooper import scoopfile.json
```

`scooper export` writes the same format back, so `scoop import` accepts
it too. Apps installed from URLs (no bucket) can't migrate automatically
and are reported as skipped. Note: each tool only manages its own shim
flavor (Scoop: `.exe`, scooper: `.cmd`), so switching managers mid-stream
can leave the other's shims behind — `checkup` flags those.

### From source

```sh
git clone https://github.com/DarbotLM/scooper
cd scooper
cargo build --release
# the binary lives at target/release/scooper.exe
```

Pinned releases and checksums are on
the [GitHub releases page](https://github.com/DarbotLM/scooper/releases).

---

## Commands

The command line interface mirrors Scoop (run `scooper help <command>` for details).

```raw
$ scooper help
Scooper is a CLI implementation of Scoop in Rust

Usage: scooper.exe [OPTIONS] <COMMAND>

Commands:
  alias        Manage command aliases
  autofetch    Fetch latest versions and preview autoupdate URLs for apps
  bucket       Manage manifest buckets
  cache        Package cache management
  cat          Inspect the manifest of a package
  checkup      Check for updates and report app status
  checkver     Check for app updates without modifying anything
  cleanup      Cleanup apps by removing old versions
  completions  Generate shell completions
  config       Configuration management
  create       Create a new Scoop bucket or manifest template
  depends      Show dependencies for a package
  download     Download a package without installing it
  export       Export list of installed apps to stdout (JSON)
  graph        Build and query Scooper's semantic repository knowledge graph
  hold         Hold package(s) to disable changes
  home         Browse the homepage of a package
  import       Import apps from an export file (or stdin) and install them
  info         Show package(s) basic information
  install      Install package(s)
  list         List installed package(s)
  prefix       Get the installation path of a package
  reset        Reset an installed package to a specific version or re-extract it
  search       Search available package(s)
  shim         Manage shims
  unhold       Unhold package(s) to enable changes
  uninstall    Uninstall package(s)
  update       Fetch and update subscribed buckets (or upgrade named apps)
  upgrade      Upgrade installed package(s)
  virustotal   Scan downloaded app archives with VirusTotal (requires API key)
  which        Find which application owns a given executable (shim)
  status       Show status of installed apps: held, upgradable, running processes
  help         Print this message or the help of a given subcommand(s)

Options:
  -v, --verbose...  Increase logging verbosity
  -q, --quiet...    Decrease logging verbosity
  -h, --help        Print help (see a summary with '-h')
```

Notable extras beyond stock Scoop parity:

- `scooper install <manifest-url|path.json>` installs isolated packages without a bucket; `app@version` (also `bucket/app@`, URL, or file pins) installs that version via bucket history or autoupdate generation
- `scooper install/upgrade/uninstall -g/--global` targets `%ProgramData%\scoop` (admin)
- `scooper upgrade --force` reinstalls even when current (previous version kept as `_<version>.old`); `scooper update <app>` forwards to `upgrade`
- `scooper install/upgrade/download --arch <32bit|64bit|arm64>` (or `SCOOP_ARCH`, or `default_architecture` config) overrides architecture resolution
- `scooper autofetch <app>|all [-w]` previews (and writes) autoupdate URL/hash refreshes
- `scooper checkver` supports `regex` (+`reverse`/`replace`), `jsonpath`, `xpath`, `script`, arch-specific specs and the `github` shorthand
- `scooper virustotal` looks up cached downloads against the VirusTotal v3 API
- Downloads use `aria2c` when available (curl fallback on missing binary or failed download; opt out with `aria2-enabled false`); manifests can be SQLite-cached via `use_sqlite_cache`
- `SCOOPER_FLAVOR=heavy` swaps progress lines for industrial ones
  (`Surveying the pit...`, `Hauling...`, `Assaying the ore...`,
  `Nothing to haul — already stockpiled.`). Off by default so scripts
  keep parsing the plain output.

See [SCOOP_FEATURES.md](SCOOP_FEATURES.md) for the full Scoop-vs-scooper feature comparison.

## DarbotLM semantic graph and agent integration

`scooper graph [ROOT]` indexes the repository tree into versioned
`scooper.semantic.v1` JSON. Nodes represent repositories, directories,
crates, source files, symbols, tests, workflows, documents, integrations, and
agents. Relationships preserve containment and source definitions with
repository-relative paths and line numbers. Generated directories such as
`target`, `node_modules`, and `dist` are excluded.

```sh
# Emit a Bitscoop-compatible graph document.
scooper graph .

# Retrieve provenance relevant to a symbol, file, or topic.
scooper graph . --query "package sync"
```

The API is available as
`scoop_rs::knowledge::SpatialKnowledgeGraph`. `agent_context` returns matches
and direct graph evidence; `bitscoop_document` and `from_bitscoop` provide the
stable interchange envelope for
[DarbotLM/bitscoop](https://github.com/DarbotLM/bitscoop). The specialized
Scooper SWE agent is read-only: it supplies code, documentation, workflow, and
test provenance and never executes package-management commands during graph
generation. Consumers should negotiate compatibility using `schema`.

## Configuration

Scooper reads the same `config.json` keys that Scoop uses, so a pre-existing Scoop
configuration is picked up automatically. Run `scooper config` to list current values or
`scooper config set <key> <value>` to change one.

```sh
# Example: redated nightly builds on every upgrade
scooper config set update_nightly true
```

Notable keys (full list in [SCOOP_FEATURES.md](SCOOP_FEATURES.md)):

| Key | Effect |
| :--- | :--- |
| `use_sqlite_cache` | Cache parsed bucket manifests in SQLite (restart faster, less disk I/O) |
| `show_manifest` | Echo each manifest JSON to the terminal before an install/upgrade confirmation prompt |
| `ignore_running_processes` | Abort install/upgrade/uninstall when an app's process is still running (otherwise prompt) |
| `aria2-*` | Enable & tune Aria2 (falls back to libcurl when missing) |
| `private_hosts` / `gh_token` | Authenticate against private GitHub repos and enterprise hosts |
| `alias` | JSON map of `scooper <name>` → real command (managed via `scooper alias`) |

## Development

```sh
# run the CLI
cargo run -- help
# run the test suite
cargo test --workspace
# lint
cargo clippy --all-targets
# release build (8.5 MB, fat-LTO, panic=abort)
cargo build --release
```

## Acknowledgments

Scooper is maintained as part of [DarbotLM](https://github.com/DarbotLM) and
grew out of [hok](https://github.com/chawyehsu/hok) by Chawye Hsu,
which laid the foundation (core library, early commands, architecture);
it was renamed, brought to full Scoop parity, and released as 1.0 from
there. Upstream behavior is defined by [Scoop](https://github.com/ScoopInstaller/Scoop).

## License

**scooper** © DarbotLM Contributors. Released under the [Apache-2.0](LICENSE) license.
For licenses of sub crates, see [COPYING](COPYING).

[license-badge]: https://img.shields.io/github/license/DarbotLM/scooper
