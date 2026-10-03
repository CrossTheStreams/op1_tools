# op1_tools

A command-line tool for saving and restoring the state of a [Teenage Engineering
OP-1](https://teenage.engineering/products/op-1) from macOS, and for dropping its
tape tracks into Ableton Live 11.

The OP-1 has no backup of its own: put it in disk mode and it appears as a USB
volume holding four folders — `album`, `drum`, `synth` and `tape`. This tool
archives those folders, writes them back, and can turn a backed-up tape into a
Live set.

## Requirements

- macOS (the mount path, `open -a` and the OP-1 itself are all macOS-specific)
- Rust 2024 edition (`cargo build`)
- Ableton Live 11 Standard at `/Applications/Ableton Live 11 Standard.app`, for
  the `live` command only

## Getting the OP-1 into disk mode

Connect it over USB, then hold **SHIFT + COM** and press **3 (DISK)**. It mounts
at `/Volumes/NO NAME`, which every command except `live` checks for first.
Eject it before unplugging.

## Build and run

```sh
cargo build --release
./target/release/op1_tools
```

Run with no arguments for an interactive menu:

```
  1) Back up the whole OP-1
  2) Restore the whole OP-1 from a backup
  3) Back up the tape only
  4) Restore the tape only from a backup
  5) Delete everything in the tape folder
  6) Open a backup's tape in Ableton Live 11
  q) Quit
```

The menu reprints whether the OP-1 is connected each time around, and a failed
action returns you to it rather than exiting — handy when you forgot disk mode.

## Commands

| Command | What it does | Store |
| --- | --- | --- |
| `backup [dir]` | Archives all four folders | `./backups` |
| `restore [dir]` | Deletes all four on the OP-1, writes a backup back | `./backups` |
| `backup-tape [dir]` | Archives `tape` only | `./tape_backups` |
| `restore-tape [dir]` | Deletes `tape` only, writes a backup back | `./tape_backups` |
| `clear-tape` | Deletes everything inside `tape` | — |
| `live` | Builds a Live set from a backup's tape and opens it | reads both stores, writes `./live_sets` |

Options: `--from NAME` picks a backup without the menu, `-y` / `--yes` skips the
confirmation, `--no-open` makes `live` build without launching Live, `-h` shows
the help.

The tape-only commands never touch `album`, `drum` or `synth`, and their backups
live in a separate directory so they can't be mistaken for a full one.

### Confirmations

Anything that deletes from the OP-1 asks first, and the word you type differs by
how much is at stake:

- `backup`, `backup-tape` — `[y/N]`
- `restore`, `restore-tape` — type `restore`
- `clear-tape` — type `delete`

`clear-tape` takes no backup of its own and cannot be undone, so it warns and
points you at `backup-tape` if your tape store is empty.

## Backups

Each backup is a gzipped tar archive named by date and increment:

```
backups/
  09-27-26-1.tar.gz      # 27 Sep 2026, first backup that day
  09-27-26-2.tar.gz      # second
tape_backups/
  10-01-26-1.tar.gz
```

Inside, paths start at the OP-1's own folders (`album/…`, `tape/track_1.aif`).
macOS clutter (`.DS_Store`, `._*`) is left out. Archives are written to a
`.partial` file and renamed only on success, so an interrupted backup never
leaves something that looks complete.

Compression helps a lot in practice, because OP-1 tape is mostly silence — a
254 MB backup of a full device came to 39 MB, and a 121 MB tape backup to 4.8 MB.

Uncompressed backups (plain folders) from before compression are still listed and
restorable; the menu marks them `(uncompressed)`.

## The `live` command

`live` is the only command that doesn't need the OP-1 connected — it reads a
backup. It lists every backup in either store, tagged `(full)` or `(tape)`, then:

1. creates `live_sets/op1-tape-<backup> Project/`, never overwriting an existing
   one (a repeat gets ` 2`, ` 3`, …),
2. extracts the four tape `.aif` files into its `Samples/Imported/`,
3. reads each file's AIFF `COMM` chunk for frame count and sample rate,
4. writes a Live 11 `.als` with one audio track per file — `Tape 1`…`Tape 4`,
   each with a single clip at bar 1, unwarped so playback is at the recorded
   speed, at 120 BPM,
5. opens it in Live.

### How the set is generated

An `.als` is gzipped XML. Rather than authoring Live's schema by hand, the
templates in `src/templates/` were extracted from a set saved by Live 11.0.12 and
scrubbed of devices, names and paths, leaving placeholders for the per-track
values. They are embedded with `include_str!`, so nothing is read from your Live
projects at runtime.

Two details worth knowing if you change this:

- Sample references use `RelativePathType="1"` (relative to the `.als` file).
  The more obvious `"3"` means *project*-relative and only resolves inside a
  folder containing `Ableton Project Info/`, which only Live itself can write —
  with `"3"` every clip fails to load with *"The file could not be opened."*
- Clip `Id` values are offset per track, since ids only need to be unique within
  a set and the template is cloned four times.

The generated folder isn't a registered Live project until you save from inside
Live. That's fine for playback, but the `.als` and its `Samples/` folder need to
move together.

## Project layout

```
src/
  main.rs            entry point and command dispatch
  cli.rs             argument parsing, --help
  menu.rs            interactive menu loop
  ui.rs              prompts, confirmations, numbered choosers
  device.rs          mount path, OP1_DIRS, Scope, mount checks
  store.rs           Backup (archive or folder), listing, naming, selection
  util.rs            recursive copy, file counting
  commands/
    backup.rs        backup and backup-tape
    restore.rs       restore and restore-tape
    tape.rs          clear-tape
    live.rs          Live set generation, AIFF parsing, launching Live
  templates/         .als XML fragments
```

`Scope` (`All` or `Tape`) is what makes one implementation serve both the full and
tape-only commands: it decides which folders are touched and which store is used,
so a tape restore can only ever delete and write `tape`.

## Dependencies

`chrono` for the local date in backup names, `flate2` for gzip, `tar` for the
archives. Nothing else.
