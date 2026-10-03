use std::fs;
use std::io;
use std::path::Path;

/// Recursively copy `src` into `dest`, returning the number of files copied.
pub(crate) fn copy_dir(src: &Path, dest: &Path) -> io::Result<usize> {
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

/// How many files `dir` holds, recursively.
pub(crate) fn count_files(dir: &Path) -> io::Result<usize> {
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
