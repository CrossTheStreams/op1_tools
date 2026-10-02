use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::Local;

/// Where the OP-1 shows up when it's mounted in disk mode.
const OP1_MOUNT: &str = "/Volumes/NO NAME";

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
           op1_tools clear-tape                   delete everything inside tape\n\n\
         `directory` holds the backups and defaults to ./{} for the full commands\n\
         and ./{} for the tape-only ones, created by a backup if missing. Each\n\
         backup inside it is a folder named MM-DD-YY-N, where N increments for each\n\
         backup taken on the same day. A restore lists what it finds there and asks\n\
         which one to use, unless `--from NAME` names it.\n\n\
         The tape commands leave album, drum and synth untouched. `clear-tape` takes\n\
         no backup of its own, so take one first if you might want the audio back.\n\n\
         options:\n  \
           --from NAME   (restore) use this backup instead of asking\n  \
           -y, --yes     skip the confirmation prompt\n  \
           -h, --help    show this help",
        Scope::All.default_dir(),
        Scope::Tape.default_dir(),
    );
}

struct Args {
    dir: PathBuf,
    assume_yes: bool,
    from: Option<String>,
}

impl Args {
    fn parse(scope: Scope, args: impl Iterator<Item = String>) -> io::Result<Self> {
        let mut dir = None;
        let mut assume_yes = false;
        let mut from = None;
        let mut args = args.peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-y" | "--yes" => assume_yes = true,
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
        })
    }

    /// The defaults an interactive menu choice runs with.
    fn interactive(scope: Scope) -> Self {
        Self {
            dir: PathBuf::from(scope.default_dir()),
            assume_yes: false,
            from: None,
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

    println!("\nBackups in {}:", dir.display());
    for (i, (name, _, _)) in backups.iter().enumerate() {
        let newest = if i + 1 == backups.len() { "  (newest)" } else { "" };
        println!("  {}) {name}{newest}", i + 1);
    }

    let answer = prompt(&format!("Which one? [1-{}] ", backups.len()))?;
    let choice: usize = answer
        .unwrap_or_default()
        .parse()
        .ok()
        .filter(|&c| c >= 1 && c <= backups.len())
        .ok_or_else(|| io::Error::other("not one of the listed backups"))?;

    Ok(dir.join(&backups[choice - 1].0))
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
