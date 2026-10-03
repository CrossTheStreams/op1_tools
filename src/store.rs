use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::device::{dirs_in, Scope};
use crate::ui::choose_row;
use crate::util::copy_dir;
use chrono::Local;

/// Extension of a compressed backup. Backups taken before compression are
/// plain folders, and are still readable.
pub(crate) const ARCHIVE_EXT: &str = ".tar.gz";

/// A backup on disk. New backups are `.tar.gz` archives; folders from before
/// compression are still read as they are.
pub(crate) enum Backup {
    Archive(PathBuf),
    Folder(PathBuf),
}

impl Backup {
    pub(crate) fn open(path: PathBuf) -> io::Result<Self> {
        if path.is_dir() {
            Ok(Backup::Folder(path))
        } else if path.is_file() {
            Ok(Backup::Archive(path))
        } else {
            Err(io::Error::other(format!(
                "no backup at '{}'",
                path.display()
            )))
        }
    }

    pub(crate) fn path(&self) -> &Path {
        match self {
            Backup::Archive(p) | Backup::Folder(p) => p,
        }
    }

    pub(crate) fn is_compressed(&self) -> bool {
        matches!(self, Backup::Archive(_))
    }

    /// Which of the scope's folders this backup holds.
    pub(crate) fn dirs_present(&self, scope: Scope) -> io::Result<Vec<&'static str>> {
        match self {
            Backup::Folder(dir) => Ok(dirs_in(dir, scope)),
            Backup::Archive(path) => {
                let mut found = Vec::new();
                for entry in self.read()?.entries()? {
                    let entry = entry?;
                    let entry_path = entry.path()?.to_path_buf();
                    let Some(top) = entry_path.components().next() else {
                        continue;
                    };
                    let top = top.as_os_str().to_string_lossy().to_string();
                    if let Some(dir) = scope.dirs().iter().find(|d| **d == top) {
                        if !found.contains(dir) {
                            found.push(*dir);
                        }
                    }
                }
                // Keep the scope's own ordering rather than the archive's.
                let _ = path;
                Ok(scope
                    .dirs()
                    .iter()
                    .copied()
                    .filter(|d| found.contains(d))
                    .collect())
            }
        }
    }

    pub(crate) fn read(&self) -> io::Result<tar::Archive<flate2::read::GzDecoder<fs::File>>> {
        let file = fs::File::open(self.path())?;
        Ok(tar::Archive::new(flate2::read::GzDecoder::new(file)))
    }

    /// Write this backup's copy of the scope's folders into `dest`, one pass for
    /// an archive, returning the file count per folder.
    pub(crate) fn extract_into(&self, scope: Scope, dest: &Path) -> io::Result<Vec<(&'static str, usize)>> {
        let mut counts: Vec<(&'static str, usize)> =
            scope.dirs().iter().map(|d| (*d, 0)).collect();

        match self {
            Backup::Folder(dir) => {
                for (name, count) in counts.iter_mut() {
                    let from = dir.join(*name);
                    if from.is_dir() {
                        *count = copy_dir(&from, &dest.join(*name))?;
                    }
                }
            }
            Backup::Archive(_) => {
                for entry in self.read()?.entries()? {
                    let mut entry = entry?;
                    let entry_path = entry.path()?.to_path_buf();
                    // Our own archives only, but never let an entry escape dest.
                    if entry_path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                    {
                        return Err(io::Error::other(format!(
                            "archive entry '{}' escapes the destination",
                            entry_path.display()
                        )));
                    }
                    let Some(top) = entry_path.components().next() else {
                        continue;
                    };
                    let top = top.as_os_str().to_string_lossy().to_string();
                    let Some(slot) = counts.iter_mut().find(|(d, _)| *d == top) else {
                        continue;
                    };
                    let target = dest.join(&entry_path);
                    if entry.header().entry_type().is_dir() {
                        fs::create_dir_all(&target)?;
                        continue;
                    }
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    entry.unpack(&target).map_err(|e| {
                        io::Error::new(e.kind(), format!("extracting {}: {e}", target.display()))
                    })?;
                    slot.1 += 1;
                }
            }
        }

        Ok(counts)
    }

    /// Put this backup's tape audio into `dest`, returning the files in track order.
    pub(crate) fn extract_tape_into(&self, dest: &Path) -> io::Result<Vec<PathBuf>> {
        match self {
            Backup::Folder(dir) => {
                let mut out = Vec::new();
                for source in tape_tracks(&dir.join("tape"))? {
                    let target = dest.join(source.file_name().unwrap_or_default());
                    fs::copy(&source, &target).map_err(|e| {
                        io::Error::new(e.kind(), format!("copying {}: {e}", source.display()))
                    })?;
                    out.push(target);
                }
                Ok(out)
            }
            Backup::Archive(_) => {
                let mut out = Vec::new();
                for entry in self.read()?.entries()? {
                    let mut entry = entry?;
                    let entry_path = entry.path()?.to_path_buf();
                    let is_tape_audio = entry_path.starts_with("tape")
                        && entry.header().entry_type().is_file()
                        && entry_path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("aif"));
                    if !is_tape_audio {
                        continue;
                    }
                    let name = entry_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if name.starts_with("._") {
                        continue;
                    }
                    let target = dest.join(&name);
                    entry.unpack(&target).map_err(|e| {
                        io::Error::new(e.kind(), format!("extracting {name}: {e}"))
                    })?;
                    out.push(target);
                }
                out.sort();
                if out.is_empty() {
                    return Err(io::Error::other(format!(
                        "'{}' has no tape audio",
                        self.path().display()
                    )));
                }
                Ok(out)
            }
        }
    }
}

/// A backup's display name, without the archive extension.
pub(crate) fn backup_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    name.strip_suffix(ARCHIVE_EXT).unwrap_or(&name).to_string()
}

/// Find a backup by name in `dir`, with or without the archive extension.
pub(crate) fn find_named(dir: &Path, name: &str) -> io::Result<PathBuf> {
    for candidate in [dir.join(format!("{name}{ARCHIVE_EXT}")), dir.join(name)] {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::other(format!(
        "no backup named '{name}' in {}",
        dir.display()
    )))
}

/// Backup archives and folders in `dir`, sorted by date then by increment.
pub(crate) fn list_backups(dir: &Path) -> io::Result<Vec<(String, u32, u32)>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_dir() && !kind.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // A half-written archive is not a backup.
        if name.ends_with(".partial") {
            continue;
        }
        if kind.is_file() && !name.ends_with(ARCHIVE_EXT) {
            continue;
        }
        if let Some((stamp, n)) = parse_backup_name(name) {
            found.push((name.to_string(), stamp, n));
        }
    }
    found.sort_by_key(|&(_, stamp, n)| (stamp, n));
    Ok(found)
}

/// `MM-DD-YY-N` into a sortable `YYMMDD` key and the increment.
fn parse_backup_name(name: &str) -> Option<(u32, u32)> {
    let name = name.strip_suffix(ARCHIVE_EXT).unwrap_or(name);
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

pub(crate) fn choose_backup(dir: &Path) -> io::Result<PathBuf> {
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
        .map(|(name, _, _)| {
            let path = dir.join(&name);
            let label = match name.strip_suffix(ARCHIVE_EXT) {
                Some(bare) => bare.to_string(),
                None => format!("{name}   (uncompressed)"),
            };
            (label, path)
        })
        .collect();
    choose_row(&format!("Backups in {}:", dir.display()), &rows)
}

/// `MM-DD-YY-N`, with `N` one past the highest already present for today.
pub(crate) fn next_backup_name(dir: &Path) -> io::Result<String> {
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

/// Every backup in either store that has a tape folder, tagged with which store
/// it came from, oldest first.
pub(crate) fn list_tape_sources() -> io::Result<Vec<(PathBuf, &'static str)>> {
    let mut found = Vec::new();
    for (scope, origin) in [(Scope::All, "full"), (Scope::Tape, "tape")] {
        let dir = Path::new(scope.default_dir());
        if !dir.is_dir() {
            continue;
        }
        for (name, stamp, n) in list_backups(dir)? {
            let path = dir.join(&name);
            // An archive's contents are only checked once it's chosen: opening
            // every one here would mean decompressing the whole store.
            let has_tape = name.ends_with(ARCHIVE_EXT) || path.join("tape").is_dir();
            if has_tape {
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
