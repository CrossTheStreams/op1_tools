use std::fs;
use std::io;
use std::path::PathBuf;

use crate::cli::Args;
use crate::device::op1_mount;
use crate::store::list_backups;
use crate::ui::{confirm, files};
use crate::util::count_files;

/// Empty the OP-1's tape folder, keeping the folder itself: the OP-1 expects it
/// to exist.
pub(crate) fn clear_tape(args: Args) -> io::Result<()> {
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
