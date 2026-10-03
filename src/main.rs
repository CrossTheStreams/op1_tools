use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use chrono::Local;

/// Where the OP-1 shows up when it's mounted in disk mode.
const OP1_MOUNT: &str = "/Volumes/NO NAME";

/// Where generated Ableton Live projects go, relative to the current directory.
const LIVE_SETS_DIR: &str = "live_sets";

/// Live 11 specifically: a Live 9 install may also be present, and it cannot
/// open a Live 11 set.
const LIVE_APP: &str = "/Applications/Ableton Live 11 Standard.app";

/// The directories the OP-1 keeps its state in.
const OP1_DIRS: [&str; 4] = ["album", "drum", "synth", "tape"];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();

    match command.as_str() {
        "backup" => backup(Scope::All, Args::parse(Scope::All, args)?),
        "restore" => restore(Scope::All, Args::parse(Scope::All, args)?),
        "backup-tape" => backup(Scope::Tape, Args::parse(Scope::Tape, args)?),
        "restore-tape" => restore(Scope::Tape, Args::parse(Scope::Tape, args)?),
        "clear-tape" => clear_tape(Args::parse(Scope::Tape, args)?),
        "live" => open_in_live(Args::parse(Scope::Tape, args)?),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "" => menu(),
        other => Err(io::Error::other(format!(
            "unknown command '{other}'. Run `op1_tools --help` for usage."
        ))),
    }
}

/// What a command operates on: everything, or just the tape.
#[derive(Clone, Copy, PartialEq)]
enum Scope {
    All,
    Tape,
}

impl Scope {
    /// The OP-1 folders this scope touches. Nothing outside this list is read,
    /// deleted or written.
    fn dirs(self) -> &'static [&'static str] {
        match self {
            Scope::All => &OP1_DIRS,
            Scope::Tape => &["tape"],
        }
    }

    /// Where this scope's backups live when no directory is given. Tape-only
    /// backups are kept apart so they can't be mistaken for a full one.
    fn default_dir(self) -> &'static str {
        match self {
            Scope::All => "backups",
            Scope::Tape => "tape_backups",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Scope::All => "OP-1",
            Scope::Tape => "tape",
        }
    }
}

fn print_help() {
    println!(
        "op1_tools - save and restore the state of an OP-1 in disk mode\n\n\
         usage:\n  \
           op1_tools                              interactive menu\n  \
           op1_tools backup       [directory]     save all four folders\n  \
           op1_tools restore      [directory]     wipe all four and write a backup back\n  \
           op1_tools backup-tape  [directory]     save only tape\n  \
           op1_tools restore-tape [directory]     wipe only tape and write a backup back\n  \
           op1_tools clear-tape                   delete everything inside tape\n  \
           op1_tools live                         open a backup's tape in Ableton Live 11\n\n\
         `directory` holds the backups and defaults to ./{} for the full commands\n\
         and ./{} for the tape-only ones, created by a backup if missing. Each\n\
         backup inside it is a folder named MM-DD-YY-N, where N increments for each\n\
         backup taken on the same day. A restore lists what it finds there and asks\n\
         which one to use, unless `--from NAME` names it.\n\n\
         The tape commands leave album, drum and synth untouched. `clear-tape` takes\n\
         no backup of its own, so take one first if you might want the audio back.\n\n\
         `live` builds a Live project under ./{} from a backup's four tape tracks,\n\
         one audio track each, and opens it. It is the only command that does not\n\
         need the OP-1 connected.\n\n\
         options:\n  \
           --from NAME   (restore) use this backup instead of asking\n  \
           -y, --yes     skip the confirmation prompt\n  \
           --no-open     (live) build the project but don't launch Live\n  \
           -h, --help    show this help",
        Scope::All.default_dir(),
        Scope::Tape.default_dir(),
        LIVE_SETS_DIR,
    );
}

struct Args {
    dir: PathBuf,
    assume_yes: bool,
    from: Option<String>,
    no_open: bool,
}

impl Args {
    fn parse(scope: Scope, args: impl Iterator<Item = String>) -> io::Result<Self> {
        let mut dir = None;
        let mut assume_yes = false;
        let mut from = None;
        let mut no_open = false;
        let mut args = args.peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-y" | "--yes" => assume_yes = true,
                "--no-open" => no_open = true,
                "--from" => {
                    from = Some(args.next().ok_or_else(|| {
                        io::Error::other("--from needs the name of a backup folder")
                    })?);
                }
                other if other.starts_with('-') => {
                    return Err(io::Error::other(format!("unknown option: {other}")));
                }
                other => {
                    if dir.is_some() {
                        return Err(io::Error::other("only one directory may be given"));
                    }
                    dir = Some(PathBuf::from(other));
                }
            }
        }

        Ok(Self {
            dir: dir.unwrap_or_else(|| PathBuf::from(scope.default_dir())),
            assume_yes,
            from,
            no_open,
        })
    }

    /// The defaults an interactive menu choice runs with.
    fn interactive(scope: Scope) -> Self {
        Self {
            dir: PathBuf::from(scope.default_dir()),
            assume_yes: false,
            from: None,
            no_open: false,
        }
    }
}

fn menu() -> io::Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "no command given and stdin is not a terminal. \
             Run `op1_tools --help` for the commands.",
        ));
    }

    println!("op1_tools");
    loop {
        match mount_status() {
            Ok(()) => println!("\nOP-1 connected at '{OP1_MOUNT}'."),
            Err(e) => println!("\n{e}"),
        }

        println!("\n  1) Back up the whole OP-1");
        println!("  2) Restore the whole OP-1 from a backup");
        println!("  3) Back up the tape only");
        println!("  4) Restore the tape only from a backup");
        println!("  5) Delete everything in the tape folder");
        println!("  6) Open a backup's tape in Ableton Live 11");
        println!("  q) Quit");

        let Some(choice) = prompt("\nChoice: ")? else {
            return Ok(());
        };

        // A failed action shouldn't drop the user out of the menu: the usual
        // cause is the OP-1 not being in disk mode yet, which they can fix and
        // then pick again.
        let result = match choice.as_str() {
            "1" => backup(Scope::All, Args::interactive(Scope::All)),
            "2" => restore(Scope::All, Args::interactive(Scope::All)),
            "3" => backup(Scope::Tape, Args::interactive(Scope::Tape)),
            "4" => restore(Scope::Tape, Args::interactive(Scope::Tape)),
            "5" => clear_tape(Args::interactive(Scope::Tape)),
            "6" => open_in_live(Args::interactive(Scope::Tape)),
            "q" | "quit" | "exit" => return Ok(()),
            other => {
                println!("'{other}' isn't one of the choices.");
                continue;
            }
        };
        if let Err(e) = result {
            println!("error: {e}");
        }
    }
}

fn backup(scope: Scope, args: Args) -> io::Result<()> {
    let mount = op1_mount()?;
    fs::create_dir_all(&args.dir).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("creating backup directory {}: {e}", args.dir.display()),
        )
    })?;

    let present = dirs_in(mount, scope);
    if present.is_empty() {
        return Err(io::Error::other(format!(
            "the OP-1 has no {} folder, so there is nothing to back up",
            scope.dirs().join(", ")
        )));
    }
    for dir in scope.dirs().iter().filter(|d| !present.contains(d)) {
        eprintln!("warning: '{dir}' not found on the OP-1, skipping");
    }

    let backup_dir = args.dir.join(next_backup_name(&args.dir)?);

    println!("\nSaving from: '{OP1_MOUNT}' ({})", present.join(", "));
    println!("Saving to:   {}", backup_dir.display());

    if !args.assume_yes && !confirm("Proceed?", "y")? {
        println!("Aborted.");
        return Ok(());
    }

    fs::create_dir(&backup_dir)?;

    let mut total = 0;
    for dir in &present {
        print!("  {dir} ... ");
        io::stdout().flush()?;
        let count = copy_dir(&mount.join(dir), &backup_dir.join(dir))?;
        println!("{}", files(count));
        total += count;
    }

    println!("Done. {} saved to {}", files(total), backup_dir.display());
    Ok(())
}

fn restore(scope: Scope, args: Args) -> io::Result<()> {
    let mount = op1_mount()?;
    if !args.dir.is_dir() {
        return Err(io::Error::other(format!(
            "no backup directory at '{}'. Take a {} backup first, \
             or pass the directory holding your backups.",
            args.dir.display(),
            scope.label()
        )));
    }

    let source = match &args.from {
        Some(name) => {
            let path = args.dir.join(name);
            if !path.is_dir() {
                return Err(io::Error::other(format!(
                    "no backup named '{name}' in {}",
                    args.dir.display()
                )));
            }
            path
        }
        None => choose_backup(&args.dir)?,
    };

    // A backup with none of the folders this scope covers would wipe the
    // device for nothing.
    let in_backup = dirs_in(&source, scope);
    if in_backup.is_empty() {
        return Err(io::Error::other(format!(
            "'{}' has no {} folder, so there is nothing to restore",
            source.display(),
            scope.dirs().join(", ")
        )));
    }
    for dir in scope.dirs().iter().filter(|d| !in_backup.contains(d)) {
        eprintln!("warning: backup has no '{dir}', it will be left empty on the OP-1");
    }

    println!("\nRestoring from: {}", source.display());
    println!("Writing to:     '{OP1_MOUNT}' ({})", in_backup.join(", "));
    println!(
        "\nThis DELETES {} on the OP-1 first. Anything there that isn't already\n\
         backed up is lost.",
        scope.dirs().join(", ")
    );
    if scope == Scope::Tape {
        println!("album, drum and synth are left untouched.");
    }

    if !args.assume_yes && !confirm("Type 'restore' to continue:", "restore")? {
        println!("Aborted.");
        return Ok(());
    }

    let mut total = 0;
    for dir in scope.dirs() {
        let on_device = mount.join(dir);
        print!("  {dir} ... ");
        io::stdout().flush()?;
        if on_device.exists() {
            fs::remove_dir_all(&on_device).map_err(|e| {
                io::Error::new(e.kind(), format!("clearing {}: {e}", on_device.display()))
            })?;
        }
        let count = if in_backup.contains(dir) {
            copy_dir(&source.join(dir), &on_device)?
        } else {
            fs::create_dir(&on_device)?;
            0
        };
        println!("{}", files(count));
        total += count;
    }

    println!("Done. {} written to the OP-1.", files(total));
    println!("Eject the OP-1 before unplugging it.");
    Ok(())
}

/// Empty the OP-1's tape folder, keeping the folder itself: the OP-1 expects it
/// to exist.
fn clear_tape(args: Args) -> io::Result<()> {
    let tape = op1_mount()?.join("tape");
    if !tape.is_dir() {
        return Err(io::Error::other("the OP-1 has no tape folder"));
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(&tape)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    if entries.is_empty() {
        println!("\nThe tape folder is already empty. Nothing to do.");
        return Ok(());
    }

    let count = count_files(&tape)?;
    println!("\nDeleting from: {}", tape.display());
    println!("Contents:      {}", files(count));
    for path in &entries {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            println!("  {name}");
        }
    }

    // Unlike a restore, there is nothing to put back afterwards.
    println!("\nThis PERMANENTLY deletes the tape audio on the OP-1. There is no undo,");
    println!("and this command takes no backup first.");
    if !args.dir.is_dir() || list_backups(&args.dir)?.is_empty() {
        println!(
            "You have no tape backups in {} yet - consider `backup-tape` first.",
            args.dir.display()
        );
    }
    println!("album, drum and synth are left untouched.");

    if !args.assume_yes && !confirm("Type 'delete' to continue:", "delete")? {
        println!("Aborted.");
        return Ok(());
    }

    for path in &entries {
        let removed = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        removed.map_err(|e| io::Error::new(e.kind(), format!("deleting {}: {e}", path.display())))?;
    }

    println!("Done. {} deleted from the tape folder.", files(count));
    println!("Eject the OP-1 before unplugging it.");
    Ok(())
}

/// How many files `dir` holds, recursively.
fn count_files(dir: &Path) -> io::Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            count += count_files(&entry.path())?;
        } else {
            count += 1;
        }
    }
    Ok(count)
}

/// Template fragments for the generated Live set, derived from a set saved by
/// Live 11.0.12 so the schema matches what Live expects exactly.
const LIVE_SET_HEAD: &str = include_str!("live/set_head.xml");
const LIVE_AUDIO_TRACK: &str = include_str!("live/audio_track.xml");
const LIVE_SET_TAIL: &str = include_str!("live/set_tail.xml");

/// The set's tempo. The tape clips are left unwarped, so this only sets the grid.
const LIVE_TEMPO: f64 = 120.0;

/// Build an Ableton Live project from a backup's tape tracks and open it.
fn open_in_live(args: Args) -> io::Result<()> {
    // The only command that doesn't touch the OP-1: it reads a backup, so the
    // device needn't be connected.
    let sources = list_tape_sources()?;
    if sources.is_empty() {
        return Err(io::Error::other(format!(
            "no backups with a tape folder found in {} or {}. Take a backup first.",
            Scope::All.default_dir(),
            Scope::Tape.default_dir()
        )));
    }

    let backup = match &args.from {
        Some(name) => {
            let matches: Vec<&(PathBuf, &str)> = sources
                .iter()
                .filter(|(path, _)| path.file_name().is_some_and(|n| n == name.as_str()))
                .collect();
            match matches.as_slice() {
                [one] => one.0.clone(),
                [] => {
                    return Err(io::Error::other(format!(
                        "no backup named '{name}' with a tape folder"
                    )));
                }
                many => {
                    return Err(io::Error::other(format!(
                        "'{name}' matches {} backups; pass the path instead",
                        many.len()
                    )));
                }
            }
        }
        None => {
            let rows: Vec<(String, PathBuf)> = sources
                .iter()
                .map(|(path, origin)| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    (format!("{name}   ({origin})"), path.clone())
                })
                .collect();
            choose_row(
                &format!(
                    "Tape in {} and {}:",
                    Scope::All.default_dir(),
                    Scope::Tape.default_dir()
                ),
                &rows,
            )?
        }
    };

    let tracks = tape_tracks(&backup.join("tape"))?;
    let name = backup.file_name().unwrap_or_default().to_string_lossy().to_string();
    let project = unused_project_dir(&format!("op1-tape-{name} Project"))?;
    let samples = project.join("Samples").join("Imported");

    println!("\nLoading tape from: {}", backup.join("tape").display());
    println!("Live project:      {}", project.display());

    fs::create_dir_all(&samples)?;

    let mut rendered = String::new();
    for (i, source) in tracks.iter().enumerate() {
        let file_name = source.file_name().unwrap_or_default();
        let copied = samples.join(file_name);
        print!("  {} ... ", file_name.to_string_lossy());
        io::stdout().flush()?;
        fs::copy(source, &copied).map_err(|e| {
            io::Error::new(e.kind(), format!("copying {}: {e}", source.display()))
        })?;

        let audio = AudioInfo::read(&copied)?;
        rendered.push_str(&render_track(i, &copied, &project, &audio)?);
        println!("{:.1}s", audio.seconds());
    }

    let als = project.join(format!("op1-tape-{name}.als"));
    write_als(&als, &format!("{LIVE_SET_HEAD}{rendered}{LIVE_SET_TAIL}"))?;

    println!(
        "Done. {} on {} audio track{} in {}",
        files(tracks.len()),
        tracks.len(),
        if tracks.len() == 1 { "" } else { "s" },
        als.display()
    );

    if args.no_open {
        return Ok(());
    }

    let app = Path::new(LIVE_APP);
    if !app.exists() {
        return Err(io::Error::other(format!(
            "Ableton Live 11 not found at '{LIVE_APP}'. The set is ready at {} - \
             open it by hand, or re-run with --no-open to skip launching Live.",
            als.display()
        )));
    }

    println!("Opening in Ableton Live 11 ...");
    let status = Command::new("open").arg("-a").arg(LIVE_APP).arg(&als).status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "`open` failed ({status}). The set is ready at {}",
            als.display()
        )));
    }
    Ok(())
}

/// Every backup in either store that has a tape folder, tagged with which store
/// it came from, oldest first.
fn list_tape_sources() -> io::Result<Vec<(PathBuf, &'static str)>> {
    let mut found = Vec::new();
    for (scope, origin) in [(Scope::All, "full"), (Scope::Tape, "tape")] {
        let dir = Path::new(scope.default_dir());
        if !dir.is_dir() {
            continue;
        }
        for (name, stamp, n) in list_backups(dir)? {
            let path = dir.join(name);
            if path.join("tape").is_dir() {
                found.push((stamp, n, path, origin));
            }
        }
    }
    found.sort_by_key(|&(stamp, n, _, _)| (stamp, n));
    Ok(found.into_iter().map(|(_, _, path, origin)| (path, origin)).collect())
}

/// The tape audio in `tape`, in track order.
fn tape_tracks(tape: &Path) -> io::Result<Vec<PathBuf>> {
    if !tape.is_dir() {
        return Err(io::Error::other(format!(
            "'{}' has no tape folder",
            tape.display()
        )));
    }
    let mut tracks: Vec<PathBuf> = fs::read_dir(tape)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("aif"))
                && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("._"))
        })
        .collect();
    tracks.sort();
    if tracks.is_empty() {
        return Err(io::Error::other(format!(
            "no .aif files in {}",
            tape.display()
        )));
    }
    Ok(tracks)
}

/// A project directory that doesn't exist yet, so an earlier set is never
/// overwritten.
fn unused_project_dir(base: &str) -> io::Result<PathBuf> {
    let root = Path::new(LIVE_SETS_DIR);
    let first = root.join(base);
    if !first.exists() {
        return Ok(first);
    }
    for n in 2..1000 {
        let candidate = root.join(format!("{base} {n}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::other(format!(
        "too many existing projects named like '{base}' in {LIVE_SETS_DIR}"
    )))
}

/// What the Live set needs to know about one audio file.
struct AudioInfo {
    frames: u64,
    rate: u32,
    size: u64,
    modified: i64,
}

impl AudioInfo {
    /// Read the AIFF/AIFC `COMM` chunk for the frame count and sample rate.
    fn read(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        let size = meta.len();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);

        let bytes = fs::read(path)?;
        let (frames, rate) = parse_aiff_comm(&bytes).ok_or_else(|| {
            io::Error::other(format!(
                "could not read the audio format of {} - is it an AIFF file?",
                path.display()
            ))
        })?;

        Ok(Self { frames, rate, size, modified })
    }

    fn seconds(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.frames as f64 / self.rate as f64
    }

    /// Clip length in beats at the set tempo.
    fn beats(&self) -> f64 {
        self.seconds() / 60.0 * LIVE_TEMPO
    }
}

/// Walk an AIFF/AIFC file's chunks for `COMM`, returning (sample frames, rate).
fn parse_aiff_comm(bytes: &[u8]) -> Option<(u64, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" {
        return None;
    }
    let form = &bytes[8..12];
    if form != b"AIFF" && form != b"AIFC" {
        return None;
    }

    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_be_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"COMM" && size >= 18 && body + 18 <= bytes.len() {
            let frames = u32::from_be_bytes(bytes[body + 2..body + 6].try_into().ok()?);
            let rate = extended_to_f64(&bytes[body + 8..body + 18])?;
            return Some((frames as u64, rate.round() as u32));
        }
        // Chunks are padded to even length.
        pos = body + size + (size % 2);
    }
    None
}

/// Decode the 80-bit IEEE 754 extended float AIFF stores its sample rate in.
fn extended_to_f64(bytes: &[u8]) -> Option<f64> {
    let exponent = u16::from_be_bytes(bytes[0..2].try_into().ok()?);
    let mantissa = u64::from_be_bytes(bytes[2..10].try_into().ok()?);
    if exponent == 0 && mantissa == 0 {
        return Some(0.0);
    }
    let sign = if exponent & 0x8000 != 0 { -1.0 } else { 1.0 };
    let unbiased = (exponent & 0x7fff) as i32 - 16383;
    Some(sign * mantissa as f64 * 2f64.powi(unbiased - 63))
}

/// Fill the audio-track template in for one tape file.
fn render_track(index: usize, audio_file: &Path, project: &Path, info: &AudioInfo) -> io::Result<String> {
    // Live's track colour indices; one each so the four tape tracks are easy to
    // tell apart in the arrangement.
    const COLORS: [u32; 4] = [10, 16, 21, 26];

    let file_name = audio_file.file_name().unwrap_or_default().to_string_lossy();
    let stem = audio_file.file_stem().unwrap_or_default().to_string_lossy();
    // track_1 -> Tape 1, anything else keeps its own name.
    let track_name = match stem.strip_prefix("track_") {
        Some(n) => format!("Tape {n}"),
        None => stem.to_string(),
    };
    let absolute = fs::canonicalize(audio_file).unwrap_or_else(|_| {
        project.join("Samples").join("Imported").join(file_name.as_ref())
    });

    let filled = LIVE_AUDIO_TRACK
        .replace("{{TRACK_NAME}}", &xml_escape(&track_name))
        .replace("{{COLOR}}", &COLORS[index % COLORS.len()].to_string())
        .replace("{{END_BEATS}}", &format!("{:.9}", info.beats()))
        .replace("{{REL_PATH}}", &xml_escape(&format!("Samples/Imported/{file_name}")))
        .replace("{{ABS_PATH}}", &xml_escape(&absolute.to_string_lossy()))
        .replace("{{FILE_SIZE}}", &info.size.to_string())
        .replace("{{LAST_MOD}}", &info.modified.to_string())
        .replace("{{FRAMES}}", &info.frames.to_string())
        .replace("{{RATE}}", &info.rate.to_string());

    // Ids are only unique within a set, so shift this track's whole subtree clear
    // of the others'.
    Ok(offset_ids(&filled, (index as u64 + 1) * 100_000))
}

/// Add `offset` to every `Id="N"` in `xml`, keeping each cloned track's internal
/// references consistent while avoiding collisions between tracks.
fn offset_ids(xml: &str, offset: u64) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find("Id=\"") {
        let (before, after) = rest.split_at(at + 4);
        out.push_str(before);
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        match digits.parse::<u64>() {
            Ok(id) => {
                out.push_str(&(id + offset).to_string());
                rest = &after[digits.len()..];
            }
            Err(_) => rest = after,
        }
    }
    out.push_str(rest);
    out
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// An .als is gzipped XML.
fn write_als(path: &Path, xml: &str) -> io::Result<()> {
    let file = fs::File::create(path)?;
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    gz.write_all(xml.as_bytes())?;
    gz.finish()?;
    Ok(())
}

/// Is the OP-1 there and does it look like an OP-1?
fn mount_status() -> io::Result<()> {
    let mount = Path::new(OP1_MOUNT);
    if !mount.is_dir() {
        return Err(io::Error::other(format!(
            "OP-1 not found at '{OP1_MOUNT}'.\n\
             Connect it over USB and hold SHIFT + COM, then press 3 (DISK) to enter disk mode."
        )));
    }
    if dirs_in(mount, Scope::All).is_empty() {
        return Err(io::Error::other(format!(
            "'{OP1_MOUNT}' is mounted but has none of the OP-1 folders ({}). \
             Is this really the OP-1?",
            OP1_DIRS.join(", ")
        )));
    }
    Ok(())
}

fn op1_mount() -> io::Result<&'static Path> {
    mount_status()?;
    Ok(Path::new(OP1_MOUNT))
}

/// Which of the scope's folders exist under `root`, in a stable order.
fn dirs_in(root: &Path, scope: Scope) -> Vec<&'static str> {
    scope
        .dirs()
        .iter()
        .copied()
        .filter(|d| root.join(d).is_dir())
        .collect()
}

/// Backup folders in `dir`, sorted by date then by increment.
fn list_backups(dir: &Path) -> io::Result<Vec<(String, u32, u32)>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some((stamp, n)) = parse_backup_name(name) {
            found.push((name.to_string(), stamp, n));
        }
    }
    found.sort_by_key(|&(_, stamp, n)| (stamp, n));
    Ok(found)
}

/// `MM-DD-YY-N` into a sortable `YYMMDD` key and the increment.
fn parse_backup_name(name: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = name.split('-').collect();
    let [month, day, year, n] = parts[..] else {
        return None;
    };
    if (month.len(), day.len(), year.len()) != (2, 2, 2) {
        return None;
    }
    let (month, day, year, n) = (
        month.parse::<u32>().ok()?,
        day.parse::<u32>().ok()?,
        year.parse::<u32>().ok()?,
        n.parse::<u32>().ok()?,
    );
    Some((year * 10_000 + month * 100 + day, n))
}

fn choose_backup(dir: &Path) -> io::Result<PathBuf> {
    let backups = list_backups(dir)?;
    if backups.is_empty() {
        return Err(io::Error::other(format!(
            "no backups found in {}. Pass the directory holding them, \
             or name one with --from.",
            dir.display()
        )));
    }

    let rows: Vec<(String, PathBuf)> = backups
        .into_iter()
        .map(|(name, _, _)| (name.clone(), dir.join(name)))
        .collect();
    choose_row(&format!("Backups in {}:", dir.display()), &rows)
}

/// Print a numbered list, oldest first with the last marked, and return the pick.
fn choose_row(heading: &str, rows: &[(String, PathBuf)]) -> io::Result<PathBuf> {
    println!("\n{heading}");
    for (i, (label, _)) in rows.iter().enumerate() {
        let newest = if i + 1 == rows.len() { "  (newest)" } else { "" };
        println!("  {}) {label}{newest}", i + 1);
    }

    let answer = prompt(&format!("Which one? [1-{}] ", rows.len()))?;
    let choice: usize = answer
        .unwrap_or_default()
        .parse()
        .ok()
        .filter(|&c| c >= 1 && c <= rows.len())
        .ok_or_else(|| io::Error::other("not one of the listed backups"))?;

    Ok(rows[choice - 1].1.clone())
}

/// `MM-DD-YY-N`, with `N` one past the highest already present for today.
fn next_backup_name(dir: &Path) -> io::Result<String> {
    let today = Local::now().format("%m-%d-%y").to_string();
    let prefix = format!("{today}-");

    let highest = list_backups(dir)?
        .iter()
        .filter(|(name, _, _)| name.starts_with(&prefix))
        .map(|&(_, _, n)| n)
        .max()
        .unwrap_or(0);

    Ok(format!("{prefix}{}", highest + 1))
}

/// Read a trimmed line, or `None` at end of input.
fn prompt(text: &str) -> io::Result<Option<String>> {
    print!("{text}");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
        println!();
        return Ok(None);
    }
    Ok(Some(answer.trim().to_string()))
}

/// Ask for confirmation, accepting only `expected` (case-insensitively).
fn confirm(text: &str, expected: &str) -> io::Result<bool> {
    let Some(answer) = prompt(&format!("{text} "))? else {
        return Ok(false);
    };
    let answer = answer.to_ascii_lowercase();
    Ok(answer == expected || (expected == "y" && answer == "yes"))
}

/// Recursively copy `src` into `dest`, returning the number of files copied.
fn copy_dir(src: &Path, dest: &Path) -> io::Result<usize> {
    fs::create_dir_all(dest)?;
    let mut count = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        // The OP-1's FAT volume picks up .DS_Store and friends; leave them out.
        if name.to_str().is_some_and(|n| n.starts_with("._") || n == ".DS_Store") {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        if entry.file_type()?.is_dir() {
            count += copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|e| io::Error::new(e.kind(), format!("copying {}: {e}", from.display())))?;
            count += 1;
        }
    }
    Ok(count)
}

fn files(count: usize) -> String {
    format!("{count} file{}", if count == 1 { "" } else { "s" })
}
