use std::fs;
use std::io::{self, Write};
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
    let mut args = std::env::args().skip(1).peekable();
    let command = args.next().unwrap_or_default();

    match command.as_str() {
        "backup" => backup(Args::parse(args)?),
        "restore" => restore(Args::parse(args)?),
        "-h" | "--help" | "help" | "" => {
            print_help();
            Ok(())
        }
        other => Err(io::Error::other(format!(
            "unknown command '{other}'. Run `op1_tools --help` for usage."
        ))),
    }
}

fn print_help() {
    println!(
        "op1_tools - save and restore the state of an OP-1 in disk mode\n\n\
         usage:\n  \
           op1_tools backup  [directory] [-y]   copy the OP-1's folders into a new backup\n  \
           op1_tools restore [directory] [-y]   wipe the OP-1's folders and write a backup back\n\n\
         `directory` holds the backups and defaults to the current directory. Each\n\
         backup is a folder named MM-DD-YY-N, where N increments for each backup\n\
         taken on the same day. `restore` lists what it finds there and asks which\n\
         one to use, unless `--from NAME` names it.\n\n\
         options:\n  \
           --from NAME   (restore) use this backup instead of asking\n  \
           -y, --yes     skip the confirmation prompt\n  \
           -h, --help    show this help"
    );
}

struct Args {
    dir: PathBuf,
    assume_yes: bool,
    from: Option<String>,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> io::Result<Self> {
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

        let dir = match dir {
            Some(d) => d,
            None => std::env::current_dir()?,
        };
        if !dir.is_dir() {
            return Err(io::Error::other(format!(
                "'{}' is not an existing directory",
                dir.display()
            )));
        }

        Ok(Self { dir, assume_yes, from })
    }
}

fn backup(args: Args) -> io::Result<()> {
    let mount = op1_mount()?;
    let present = op1_dirs_in(mount)?;
    for dir in OP1_DIRS.iter().filter(|d| !present.contains(d)) {
        eprintln!("warning: '{dir}' not found on the OP-1, skipping");
    }

    let backup_dir = args.dir.join(next_backup_name(&args.dir)?);

    println!("OP-1 found at '{OP1_MOUNT}'");
    println!("Folders to save: {}", present.join(", "));
    println!("Saving to:       {}", backup_dir.display());

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

fn restore(args: Args) -> io::Result<()> {
    let mount = op1_mount()?;

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

    // A backup with none of the OP-1's folders would wipe the device for nothing.
    let in_backup = op1_dirs_in(&source)?;
    for dir in OP1_DIRS.iter().filter(|d| !in_backup.contains(d)) {
        eprintln!("warning: backup has no '{dir}', it will be left empty on the OP-1");
    }

    println!("OP-1 found at '{OP1_MOUNT}'");
    println!("Restoring from:  {}", source.display());
    println!("Folders to write: {}", in_backup.join(", "));
    println!(
        "\nThis DELETES {} on the OP-1 first. Anything not already backed up is lost.",
        OP1_DIRS.join(", ")
    );

    if !args.assume_yes && !confirm("Type 'restore' to continue:", "restore")? {
        println!("Aborted.");
        return Ok(());
    }

    let mut total = 0;
    for dir in OP1_DIRS {
        let on_device = mount.join(dir);
        print!("  {dir} ... ");
        io::stdout().flush()?;
        if on_device.exists() {
            fs::remove_dir_all(&on_device).map_err(|e| {
                io::Error::new(e.kind(), format!("clearing {}: {e}", on_device.display()))
            })?;
        }
        let count = if in_backup.contains(&dir) {
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

fn op1_mount() -> io::Result<&'static Path> {
    let mount = Path::new(OP1_MOUNT);
    if !mount.is_dir() {
        return Err(io::Error::other(format!(
            "OP-1 not found at '{OP1_MOUNT}'.\n\
             Connect it over USB and hold SHIFT + COM, then press 3 (DISK) to enter disk mode."
        )));
    }
    if op1_dirs_in(mount)?.is_empty() {
        return Err(io::Error::other(format!(
            "'{OP1_MOUNT}' is mounted but has none of the OP-1 folders ({}). \
             Is this really the OP-1?",
            OP1_DIRS.join(", ")
        )));
    }
    Ok(mount)
}

/// Which of the OP-1's folders exist under `root`, in a stable order.
fn op1_dirs_in(root: &Path) -> io::Result<Vec<&'static str>> {
    let present: Vec<&str> = OP1_DIRS
        .iter()
        .copied()
        .filter(|d| root.join(d).is_dir())
        .collect();
    if present.is_empty() && root != Path::new(OP1_MOUNT) {
        return Err(io::Error::other(format!(
            "'{}' has none of the OP-1 folders ({}), so there is nothing to restore",
            root.display(),
            OP1_DIRS.join(", ")
        )));
    }
    Ok(present)
}

/// Backup folders in `dir`, newest-looking last: sorted by date, then by increment.
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

    println!("Backups in {}:", dir.display());
    for (i, (name, _, _)) in backups.iter().enumerate() {
        let newest = if i + 1 == backups.len() { "  (newest)" } else { "" };
        println!("  {}) {name}{newest}", i + 1);
    }

    print!("Which one? [1-{}] ", backups.len());
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let choice: usize = answer
        .trim()
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

/// Ask for confirmation, accepting only `expected` (case-insensitively).
fn confirm(prompt: &str, expected: &str) -> io::Result<bool> {
    print!("{prompt} ");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
        return Ok(false);
    }
    let answer = answer.trim().to_ascii_lowercase();
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
