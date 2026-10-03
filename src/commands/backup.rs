use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::Args;
use crate::device::{dirs_in, op1_mount, Scope, OP1_MOUNT};
use crate::store::{next_backup_name, ARCHIVE_EXT};
use crate::ui::{confirm, files};

pub(crate) fn backup(scope: Scope, args: Args) -> io::Result<()> {
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

    let archive = args
        .dir
        .join(format!("{}{ARCHIVE_EXT}", next_backup_name(&args.dir)?));

    println!("\nSaving from: '{OP1_MOUNT}' ({})", present.join(", "));
    println!("Saving to:   {}", archive.display());

    if !args.assume_yes && !confirm("Proceed?", "y")? {
        println!("Aborted.");
        return Ok(());
    }

    // Write to a partial file first: an interrupted backup then leaves no
    // archive behind that looks complete.
    let partial = archive.with_extension("partial");
    let mut total = 0;
    let result = (|| -> io::Result<()> {
        let file = fs::File::create(&partial)?;
        // Audio barely compresses, so the fast setting buys most of the saving
        // for a fraction of the time.
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for dir in &present {
            print!("  {dir} ... ");
            io::stdout().flush()?;
            let count = append_dir(&mut tar, &mount.join(dir), Path::new(dir))?;
            println!("{}", files(count));
            total += count;
        }
        tar.into_inner()?.finish()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    fs::rename(&partial, &archive)?;

    println!(
        "Done. {} saved to {} ({}).",
        files(total),
        archive.display(),
        size_of(&archive)
    );
    Ok(())
}

/// Add `src` to the archive under `prefix`, returning the number of files.
fn append_dir<W: Write>(
    tar: &mut tar::Builder<W>,
    src: &Path,
    prefix: &Path,
) -> io::Result<usize> {
    let mut count = 0;
    let mut entries: Vec<PathBuf> = fs::read_dir(src)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();

    tar.append_dir(prefix, src)?;
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        // The OP-1's FAT volume picks up .DS_Store and friends; leave them out.
        if name.starts_with("._") || name == ".DS_Store" {
            continue;
        }
        let inner = prefix.join(&name);
        if path.is_dir() {
            count += append_dir(tar, &path, &inner)?;
        } else {
            tar.append_path_with_name(&path, &inner)
                .map_err(|e| io::Error::new(e.kind(), format!("archiving {}: {e}", path.display())))?;
            count += 1;
        }
    }
    Ok(count)
}

/// A human-readable size for a finished archive.
fn size_of(path: &Path) -> String {
    let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0) as f64;
    for (limit, unit) in [(1e9, "GB"), (1e6, "MB"), (1e3, "KB")] {
        if bytes >= limit {
            return format!("{:.1} {unit}", bytes / limit);
        }
    }
    format!("{bytes:.0} bytes")
}
