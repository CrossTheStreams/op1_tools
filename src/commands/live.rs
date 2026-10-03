use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::Args;
use crate::device::Scope;
use crate::store::{backup_name, list_tape_sources, Backup, ARCHIVE_EXT};
use crate::ui::{choose_row, files};

/// Where generated Ableton Live projects go, relative to the current directory.
pub(crate) const LIVE_SETS_DIR: &str = "live_sets";

/// Live 11 specifically: a Live 9 install may also be present, and it cannot
/// open a Live 11 set.
const LIVE_APP: &str = "/Applications/Ableton Live 11 Standard.app";

/// Template fragments for the generated Live set, derived from a set saved by
/// Live 11.0.12 so the schema matches what Live expects exactly.
const LIVE_SET_HEAD: &str = include_str!("../templates/set_head.xml");

const LIVE_AUDIO_TRACK: &str = include_str!("../templates/audio_track.xml");

const LIVE_SET_TAIL: &str = include_str!("../templates/set_tail.xml");

/// The set's tempo. The tape clips are left unwarped, so this only sets the grid.
const LIVE_TEMPO: f64 = 120.0;

/// Build an Ableton Live project from a backup's tape tracks and open it.
pub(crate) fn open_in_live(args: Args) -> io::Result<()> {
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
            let wanted = [name.clone(), format!("{name}{ARCHIVE_EXT}")];
            let matches: Vec<&(PathBuf, &str)> = sources
                .iter()
                .filter(|(path, _)| {
                    path.file_name()
                        .is_some_and(|n| wanted.iter().any(|w| n == w.as_str()))
                })
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
                    (format!("{}   ({origin})", backup_name(path)), path.clone())
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

    let source = Backup::open(backup)?;
    let name = backup_name(source.path());
    let project = unused_project_dir(&format!("op1-tape-{name} Project"))?;
    let samples = project.join("Samples").join("Imported");

    println!("\nLoading tape from: {}", source.path().display());
    println!("Live project:      {}", project.display());

    fs::create_dir_all(&samples)?;

    print!("  {} tape ... ", if source.is_compressed() { "decompressing" } else { "copying" });
    io::stdout().flush()?;
    let tracks = source.extract_tape_into(&samples)?;
    println!("{}", files(tracks.len()));

    let mut rendered = String::new();
    for (i, copied) in tracks.iter().enumerate() {
        let audio = AudioInfo::read(copied)?;
        println!(
            "  {} ... {:.1}s",
            copied.file_name().unwrap_or_default().to_string_lossy(),
            audio.seconds()
        );
        rendered.push_str(&render_track(i, copied, &project, &audio)?);
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
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
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
