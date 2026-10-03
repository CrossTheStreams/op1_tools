use std::io;
use std::path::PathBuf;

use crate::device::Scope;
use crate::commands::live::LIVE_SETS_DIR;
use crate::store::ARCHIVE_EXT;

pub(crate) fn print_help() {
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
         backup inside it is a compressed archive named MM-DD-YY-N{}, where N\n\
         increments for each backup taken on the same day. Restores and `live`\n\
         decompress as needed, and still read uncompressed backups taken before\n\
         compression. A restore lists what it finds and asks which one to use,\n\
         unless `--from NAME` names it (the extension is optional there).\n\n\
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
        ARCHIVE_EXT,
        LIVE_SETS_DIR,
    );
}

pub(crate) struct Args {
    pub(crate) dir: PathBuf,
    pub(crate) assume_yes: bool,
    pub(crate) from: Option<String>,
    pub(crate) no_open: bool,
}

impl Args {
    pub(crate) fn parse(scope: Scope, args: impl Iterator<Item = String>) -> io::Result<Self> {
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
    pub(crate) fn interactive(scope: Scope) -> Self {
        Self {
            dir: PathBuf::from(scope.default_dir()),
            assume_yes: false,
            from: None,
            no_open: false,
        }
    }
}
