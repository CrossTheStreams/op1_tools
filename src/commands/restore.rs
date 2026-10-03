use std::fs;
use std::io::{self, Write};

use crate::cli::Args;
use crate::device::{op1_mount, Scope, OP1_MOUNT};
use crate::store::{choose_backup, find_named, Backup};
use crate::ui::{confirm, files};

pub(crate) fn restore(scope: Scope, args: Args) -> io::Result<()> {
    let mount = op1_mount()?;
    if !args.dir.is_dir() {
        return Err(io::Error::other(format!(
            "no backup directory at '{}'. Take a {} backup first, \
             or pass the directory holding your backups.",
            args.dir.display(),
            scope.label()
        )));
    }

    let source = Backup::open(match &args.from {
        Some(name) => find_named(&args.dir, name)?,
        None => choose_backup(&args.dir)?,
    })?;

    // A backup with none of the folders this scope covers would wipe the
    // device for nothing.
    if source.is_compressed() {
        println!("\nReading {} ...", source.path().display());
    }
    let in_backup = source.dirs_present(scope)?;
    if in_backup.is_empty() {
        return Err(io::Error::other(format!(
            "'{}' has no {} folder, so there is nothing to restore",
            source.path().display(),
            scope.dirs().join(", ")
        )));
    }
    for dir in scope.dirs().iter().filter(|d| !in_backup.contains(d)) {
        eprintln!("warning: backup has no '{dir}', it will be left empty on the OP-1");
    }

    println!("\nRestoring from: {}", source.path().display());
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

    // Clear every folder in scope before writing, so a restore can't leave a
    // mix of old and new files behind.
    for dir in scope.dirs() {
        let on_device = mount.join(dir);
        if on_device.exists() {
            fs::remove_dir_all(&on_device).map_err(|e| {
                io::Error::new(e.kind(), format!("clearing {}: {e}", on_device.display()))
            })?;
        }
        fs::create_dir(&on_device)?;
    }

    print!("  writing ... ");
    io::stdout().flush()?;
    let counts = source.extract_into(scope, mount)?;
    println!("done");

    let mut total = 0;
    for (dir, count) in &counts {
        println!("  {dir} ... {}", files(*count));
        total += count;
    }

    println!("Done. {} written to the OP-1.", files(total));
    println!("Eject the OP-1 before unplugging it.");
    Ok(())
}
