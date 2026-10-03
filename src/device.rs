use std::io;
use std::path::Path;

/// Where the OP-1 shows up when it's mounted in disk mode.
pub(crate) const OP1_MOUNT: &str = "/Volumes/NO NAME";

/// The directories the OP-1 keeps its state in.
const OP1_DIRS: [&str; 4] = ["album", "drum", "synth", "tape"];

/// What a command operates on: everything, or just the tape.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Scope {
    All,
    Tape,
}

impl Scope {
    /// The OP-1 folders this scope touches. Nothing outside this list is read,
    /// deleted or written.
    pub(crate) fn dirs(self) -> &'static [&'static str] {
        match self {
            Scope::All => &OP1_DIRS,
            Scope::Tape => &["tape"],
        }
    }

    /// Where this scope's backups live when no directory is given. Tape-only
    /// backups are kept apart so they can't be mistaken for a full one.
    pub(crate) fn default_dir(self) -> &'static str {
        match self {
            Scope::All => "backups",
            Scope::Tape => "tape_backups",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Scope::All => "OP-1",
            Scope::Tape => "tape",
        }
    }
}

/// Is the OP-1 there and does it look like an OP-1?
pub(crate) fn mount_status() -> io::Result<()> {
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

pub(crate) fn op1_mount() -> io::Result<&'static Path> {
    mount_status()?;
    Ok(Path::new(OP1_MOUNT))
}

/// Which of the scope's folders exist under `root`, in a stable order.
pub(crate) fn dirs_in(root: &Path, scope: Scope) -> Vec<&'static str> {
    scope
        .dirs()
        .iter()
        .copied()
        .filter(|d| root.join(d).is_dir())
        .collect()
}
