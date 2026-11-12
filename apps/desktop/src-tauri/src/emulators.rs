//! Emulators and ROMs. The user names folders of ROMs per system (settings
//! `emulators.romFolders`) and the emulator for each; a scan adds every ROM as
//! a library game (title from the file name, a collection per system). A ROM
//! in such a folder starts through its emulator (`command`); everything else
//! about a game (playtime, game options, backups) works as usual.
//!
//! Cove brings no emulator, BIOS or ROM; it finds emulators the user
//! installed (program in `PATH`, Flatpak, the usual install folders).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::models::{EmulatorInfo, EmulatorSettings, Platform, RomFolder, SystemInfo};

/// An emulator: how to find it and how to start a ROM with it.
#[derive(Debug, Clone, Copy)]
pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// Program names in `PATH` (Linux, macOS; Windows with `.exe`).
    programs: &'static [&'static str],
    flatpak: Option<&'static str>,
    /// Windows: below `Program Files`; macOS: the app bundle's program.
    windows: &'static [&'static str],
    macos: &'static [&'static str],
    /// `{rom}` = the ROM, `{core}` = the `RetroArch` core.
    args: &'static [&'static str],
}

const RETROARCH: Emulator = Emulator {
    id: "retroarch",
    name: "RetroArch",
    programs: &["retroarch"],
    flatpak: Some("org.libretro.RetroArch"),
    windows: &["RetroArch-Win64/retroarch.exe", "RetroArch/retroarch.exe"],
    macos: &["/Applications/RetroArch.app/Contents/MacOS/RetroArch"],
    args: &["-f", "-L", "{core}", "{rom}"],
};
const DOLPHIN: Emulator = Emulator {
    id: "dolphin",
    name: "Dolphin",
    programs: &["dolphin-emu"],
    flatpak: Some("org.DolphinEmu.dolphin-emu"),
    windows: &["Dolphin/Dolphin.exe", "Dolphin-x64/Dolphin.exe"],
    macos: &["/Applications/Dolphin.app/Contents/MacOS/Dolphin"],
    args: &["-b", "-e", "{rom}"],
};
const PCSX2: Emulator = Emulator {
    id: "pcsx2",
    name: "PCSX2",
    programs: &["pcsx2-qt", "pcsx2"],
    flatpak: Some("net.pcsx2.PCSX2"),
    windows: &["PCSX2/pcsx2-qt.exe"],
    macos: &["/Applications/PCSX2.app/Contents/MacOS/PCSX2"],
    args: &["-batch", "-fullscreen", "--", "{rom}"],
};
const DUCKSTATION: Emulator = Emulator {
    id: "duckstation",
    name: "DuckStation",
    programs: &["duckstation-qt", "duckstation"],
    flatpak: Some("org.duckstation.DuckStation"),
    windows: &["DuckStation/duckstation-qt-x64-ReleaseLTCG.exe"],
    macos: &["/Applications/DuckStation.app/Contents/MacOS/DuckStation"],
    args: &["-batch", "-fullscreen", "--", "{rom}"],
};
const PPSSPP: Emulator = Emulator {
    id: "ppsspp",
    name: "PPSSPP",
    programs: &["PPSSPPSDL", "ppsspp"],
    flatpak: Some("org.ppsspp.PPSSPP"),
    windows: &["PPSSPP/PPSSPPWindows64.exe"],
    macos: &["/Applications/PPSSPPSDL.app/Contents/MacOS/PPSSPPSDL"],
    args: &["--fullscreen", "{rom}"],
};
const MGBA: Emulator = Emulator {
    id: "mgba",
    name: "mGBA",
    programs: &["mgba-qt", "mgba"],
    flatpak: Some("io.mgba.mGBA"),
    windows: &["mGBA/mGBA.exe"],
    macos: &["/Applications/mGBA.app/Contents/MacOS/mGBA"],
    args: &["-f", "{rom}"],
};
const MELONDS: Emulator = Emulator {
    id: "melonds",
    name: "melonDS",
    programs: &["melonDS"],
    flatpak: Some("net.kuribo64.melonDS"),
    windows: &["melonDS/melonDS.exe"],
    macos: &["/Applications/melonDS.app/Contents/MacOS/melonDS"],
    args: &["{rom}"],
};
const FLYCAST: Emulator = Emulator {
    id: "flycast",
    name: "Flycast",
    programs: &["flycast"],
    flatpak: Some("org.flycast.Flycast"),
    windows: &["Flycast/flycast.exe"],
    macos: &["/Applications/Flycast.app/Contents/MacOS/Flycast"],
    args: &["{rom}"],
};

/// A console Cove knows: its ROM files and the emulators for it.
#[derive(Debug, Clone, Copy)]
pub struct System {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// The `RetroArch` core (`<core>_libretro.so|dll|dylib`).
    pub core: &'static str,
    /// Standalone emulators; `RetroArch` always works.
    pub emulators: &'static [Emulator],
}

pub const SYSTEMS: &[System] = &[
    System {
        id: "nes",
        name: "NES",
        extensions: &["nes", "fds", "unf", "zip"],
        core: "mesen",
        emulators: &[],
    },
    System {
        id: "snes",
        name: "Super Nintendo",
        extensions: &["sfc", "smc", "zip"],
        core: "snes9x",
        emulators: &[],
    },
    System {
        id: "n64",
        name: "Nintendo 64",
        extensions: &["n64", "z64", "v64", "zip"],
        core: "mupen64plus_next",
        emulators: &[],
    },
    System {
        id: "gb",
        name: "Game Boy",
        extensions: &["gb", "zip"],
        core: "gambatte",
        emulators: &[MGBA],
    },
    System {
        id: "gbc",
        name: "Game Boy Color",
        extensions: &["gbc", "zip"],
        core: "gambatte",
        emulators: &[MGBA],
    },
    System {
        id: "gba",
        name: "Game Boy Advance",
        extensions: &["gba", "zip"],
        core: "mgba",
        emulators: &[MGBA],
    },
    System {
        id: "nds",
        name: "Nintendo DS",
        extensions: &["nds"],
        core: "melonds",
        emulators: &[MELONDS],
    },
    System {
        id: "gamecube",
        name: "GameCube",
        extensions: &["iso", "gcm", "gcz", "rvz", "ciso"],
        core: "dolphin",
        emulators: &[DOLPHIN],
    },
    System {
        id: "wii",
        name: "Wii",
        extensions: &["iso", "wbfs", "rvz", "wad"],
        core: "dolphin",
        emulators: &[DOLPHIN],
    },
    System {
        id: "ps1",
        name: "PlayStation",
        extensions: &["cue", "chd", "pbp", "m3u"],
        core: "swanstation",
        emulators: &[DUCKSTATION],
    },
    System {
        id: "ps2",
        name: "PlayStation 2",
        extensions: &["iso", "chd", "cso"],
        core: "pcsx2",
        emulators: &[PCSX2],
    },
    System {
        id: "psp",
        name: "PSP",
        extensions: &["iso", "cso", "pbp"],
        core: "ppsspp",
        emulators: &[PPSSPP],
    },
    System {
        id: "genesis",
        name: "Mega Drive / Genesis",
        extensions: &["md", "gen", "smd", "bin", "zip"],
        core: "genesis_plus_gx",
        emulators: &[],
    },
    System {
        id: "mastersystem",
        name: "Master System",
        extensions: &["sms", "zip"],
        core: "genesis_plus_gx",
        emulators: &[],
    },
    System {
        id: "dreamcast",
        name: "Dreamcast",
        extensions: &["gdi", "cdi", "chd"],
        core: "flycast",
        emulators: &[FLYCAST],
    },
];

pub const CUSTOM: &str = "custom";

pub fn system(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

fn emulator(system: &System, id: &str) -> Option<Emulator> {
    if id == RETROARCH.id {
        return Some(RETROARCH);
    }
    system.emulators.iter().copied().find(|e| e.id == id)
}

/// Checks the ROM folders before they are saved.
pub fn validate(settings: &EmulatorSettings) -> AppResult<()> {
    let invalid = |m: String| Err(AppError::InvalidInput(m));
    if settings.rom_folders.len() > 100 {
        return invalid(tr!("Höchstens 100 ROM-Ordner.", "At most 100 ROM folders."));
    }
    let mut ids = HashSet::new();
    for folder in &settings.rom_folders {
        let id_ok = !folder.id.is_empty()
            && folder.id.len() <= 64
            && folder
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !id_ok || !ids.insert(folder.id.as_str()) {
            return invalid(tr!("Ungültige ROM-Ordner-ID.", "Invalid ROM folder ID."));
        }
        let Some(system) = system(&folder.system) else {
            return invalid(tr!("Unbekanntes System.", "Unknown system."));
        };
        if folder.path.trim().is_empty() {
            return invalid(tr!(
                "Bitte gib einen ROM-Ordner an.",
                "Please enter a ROM folder."
            ));
        }
        if folder.emulator == CUSTOM {
            let program = folder.program.as_deref().map(str::trim).unwrap_or_default();
            if program.is_empty() || !folder.arguments.contains("{rom}") {
                return invalid(tr!(
                    "Ein eigenes Programm braucht einen Pfad und „{{rom}}“ in den Argumenten.",
                    "Your own program needs a path and “{{rom}}” in its arguments."
                ));
            }
        } else if emulator(system, &folder.emulator).is_none() {
            return invalid(tr!(
                "Dieser Emulator passt nicht zu {}.",
                "This emulator doesn't fit {}.",
                system.name
            ));
        }
    }
    Ok(())
}

// ───────────── finding emulators ─────────────

/// Where to look; overridable in tests.
#[derive(Debug, Clone)]
pub struct Env {
    pub platform: Platform,
    pub home: Option<PathBuf>,
    /// `PATH` entries.
    pub path: Vec<PathBuf>,
    /// `Program Files` folders (Windows).
    pub program_files: Vec<PathBuf>,
}

impl Env {
    pub fn detect(home: Option<PathBuf>) -> Self {
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let program_files = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
            .chain(std::iter::once(PathBuf::from("C:\\")))
            .collect();
        Self {
            platform: Platform::current(),
            home,
            path,
            program_files,
        }
    }

    fn which(&self, name: &str) -> Option<PathBuf> {
        let name = if self.platform == Platform::Windows {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        self.path
            .iter()
            .map(|dir| dir.join(&name))
            .find(|p| p.is_file())
    }

    fn flatpak_installed(&self, app: &str) -> bool {
        let user = self
            .home
            .as_ref()
            .map(|h| h.join(".local/share/flatpak/app").join(app));
        user.into_iter()
            .chain(std::iter::once(
                PathBuf::from("/var/lib/flatpak/app").join(app),
            ))
            .any(|p| p.is_dir())
    }
}

/// The command to run an emulator (program, then arguments before its own).
fn find(emulator: &Emulator, env: &Env) -> Option<Vec<String>> {
    let text = |p: PathBuf| p.to_string_lossy().into_owned();
    if let Some(found) = emulator.programs.iter().find_map(|p| env.which(p)) {
        return Some(vec![text(found)]);
    }
    match env.platform {
        Platform::Linux => {
            let app = emulator.flatpak?;
            let flatpak = env.which("flatpak")?;
            env.flatpak_installed(app)
                .then(|| vec![text(flatpak), "run".into(), app.into()])
        }
        Platform::Windows => env
            .program_files
            .iter()
            .flat_map(|dir| emulator.windows.iter().map(move |rel| dir.join(rel)))
            .find(|p| p.is_file())
            .map(|p| vec![text(p)]),
        Platform::Macos => emulator
            .macos
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
            .map(|p| vec![text(p)]),
    }
}

/// A `RetroArch` core: in the user's or the system's core folders.
fn find_core(core: &str, command: &[String], env: &Env) -> Option<PathBuf> {
    let file = match env.platform {
        Platform::Windows => format!("{core}_libretro.dll"),
        Platform::Macos => format!("{core}_libretro.dylib"),
        Platform::Linux => format!("{core}_libretro.so"),
    };
    let home = env.home.clone().unwrap_or_default();
    let mut dirs = vec![
        home.join(".config/retroarch/cores"),
        home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores"),
        home.join("Library/Application Support/RetroArch/cores"),
        PathBuf::from("/usr/lib/libretro"),
        PathBuf::from("/usr/lib64/libretro"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu/libretro"),
        PathBuf::from("/usr/lib/aarch64-linux-gnu/libretro"),
        PathBuf::from("/usr/local/lib/libretro"),
    ];
    // Windows: next to retroarch.exe.
    if let Some(dir) = command.first().map(Path::new).and_then(Path::parent) {
        dirs.insert(0, dir.join("cores"));
    }
    dirs.into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
}

/// Every system with its emulators (`RetroArch` first). Blocking.
pub fn systems(env: &Env) -> Vec<SystemInfo> {
    let retroarch = find(&RETROARCH, env).is_some();
    SYSTEMS
        .iter()
        .map(|s| SystemInfo {
            id: s.id.into(),
            name: s.name.into(),
            extensions: s.extensions.iter().map(|e| (*e).to_owned()).collect(),
            emulators: std::iter::once(EmulatorInfo {
                id: RETROARCH.id.into(),
                name: RETROARCH.name.into(),
                installed: retroarch,
            })
            .chain(s.emulators.iter().map(|e| EmulatorInfo {
                id: e.id.into(),
                name: e.name.into(),
                installed: find(e, env).is_some(),
            }))
            .collect(),
        })
        .collect()
}

// ───────────── starting a ROM ─────────────

/// The ROM folder a file belongs to.
pub fn folder_of<'a>(folders: &'a [RomFolder], file: &Path) -> Option<&'a RomFolder> {
    folders
        .iter()
        .filter(|f| !f.path.trim().is_empty())
        .find(|f| file.starts_with(Path::new(f.path.trim())))
}

fn not_installed(name: &str) -> AppError {
    AppError::Launch(tr!(
        "{name} ist nicht installiert. Installiere es oder wähle unter Einstellungen → Emulatoren ein anderes Programm.",
        "{name} is not installed. Install it or choose another program under Settings → Emulators."
    ))
}

/// Program and arguments that start `rom` from `folder`.
pub fn command(folder: &RomFolder, rom: &Path, env: &Env) -> AppResult<(PathBuf, Vec<String>)> {
    let rom_text = rom.to_string_lossy().into_owned();
    let system = system(&folder.system)
        .ok_or_else(|| AppError::Launch(tr!("Unbekanntes System.", "Unknown system.")))?;
    let (mut command, args): (Vec<String>, Vec<String>) = if folder.emulator == CUSTOM {
        let program = folder.program.as_deref().map(str::trim).unwrap_or_default();
        if program.is_empty() {
            return Err(not_installed("?"));
        }
        let args = shell_words(&folder.arguments)?
            .into_iter()
            .map(|a| a.replace("{rom}", &rom_text))
            .collect();
        (vec![program.to_owned()], args)
    } else {
        let emulator =
            emulator(system, &folder.emulator).ok_or_else(|| not_installed(&folder.emulator))?;
        let command = match folder
            .program
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            Some(program) => vec![program.to_owned()],
            None => find(&emulator, env).ok_or_else(|| not_installed(emulator.name))?,
        };
        let core = if emulator.args.contains(&"{core}") {
            let core = find_core(system.core, &command, env).ok_or_else(|| {
                AppError::Launch(tr!(
                    "Der RetroArch-Core „{}“ fehlt. Installiere ihn in RetroArch (Hauptmenü → Online-Updater → Cores).",
                    "The RetroArch core “{}” is missing. Install it in RetroArch (Main Menu → Online Updater → Cores).",
                    system.core
                ))
            })?;
            core.to_string_lossy().into_owned()
        } else {
            String::new()
        };
        let args = emulator
            .args
            .iter()
            .map(|a| a.replace("{rom}", &rom_text).replace("{core}", &core))
            .collect();
        (command, args)
    };
    let program = PathBuf::from(command.remove(0));
    command.extend(args);
    Ok((program, command))
}

/// Splits arguments like a shell (quotes keep spaces).
fn shell_words(raw: &str) -> AppResult<Vec<String>> {
    shlex::split(raw).ok_or_else(|| {
        AppError::InvalidInput(tr!(
            "Die Argumente haben ein offenes Anführungszeichen.",
            "The arguments have an unclosed quote."
        ))
    })
}

// ───────────── scanning ─────────────

/// A ROM found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rom {
    pub path: PathBuf,
    pub title: String,
    pub size: u64,
}

/// "Super Game (USA) (Rev 1) [!].sfc" → "Super Game".
pub fn title_of(file: &Path) -> String {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = String::new();
    let mut depth = 0usize;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(if c == '_' { ' ' } else { c }),
            _ => {}
        }
    }
    let title = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        stem
    } else {
        title
    }
}

/// The ROMs of a folder (three levels deep). Discs listed in an `.m3u` are
/// left out, the playlist stands for them. Blocking.
pub fn scan(folder: &RomFolder) -> Vec<Rom> {
    let Some(system) = system(&folder.system) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect(Path::new(folder.path.trim()), 3, &mut files);
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_lowercase());
    let mut in_playlists = HashSet::new();
    for file in files.iter().filter(|f| ext(f).as_deref() == Some("m3u")) {
        if let Ok(text) = std::fs::read_to_string(file) {
            for line in text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
            {
                if let Some(dir) = file.parent() {
                    in_playlists.insert(dir.join(line));
                }
            }
        }
    }
    let mut roms: Vec<Rom> = files
        .into_iter()
        .filter(|f| ext(f).is_some_and(|e| system.extensions.contains(&e.as_str())))
        .filter(|f| !in_playlists.contains(f))
        .map(|path| Rom {
            title: title_of(&path),
            size: std::fs::metadata(&path).map_or(0, |m| m.len()),
            path,
        })
        .collect();
    roms.sort_by(|a, b| a.path.cmp(&b.path));
    roms
}

fn collect(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() {
            out.push(entry.path());
        } else if kind.is_dir() && depth > 0 {
            collect(&entry.path(), depth - 1, out);
        }
        if out.len() > 50_000 {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(system: &str, path: &Path, emulator: &str) -> RomFolder {
        RomFolder {
            id: format!("f-{system}"),
            system: system.into(),
            path: path.to_string_lossy().into_owned(),
            emulator: emulator.into(),
            program: None,
            arguments: String::new(),
        }
    }

    fn env(root: &Path) -> Env {
        Env {
            platform: Platform::Linux,
            home: Some(root.join("home")),
            path: vec![root.join("bin")],
            program_files: Vec::new(),
        }
    }

    #[test]
    fn titles_lose_their_tags() {
        assert_eq!(
            title_of(Path::new("/r/Super Game (USA) (Rev 1) [!].sfc")),
            "Super Game"
        );
        assert_eq!(
            title_of(Path::new("/r/Space_Quest_II.smc")),
            "Space Quest II"
        );
        assert_eq!(title_of(Path::new("/r/(Prototype).nes")), "(Prototype)");
    }

    #[test]
    fn scans_roms_and_leaves_playlist_discs_out() {
        let root = tempfile::tempdir().unwrap();
        let ps1 = root.path().join("ps1");
        std::fs::create_dir_all(ps1.join("Big Game")).unwrap();
        std::fs::write(ps1.join("Big Game/Big Game (Disc 1).cue"), b"x").unwrap();
        std::fs::write(ps1.join("Big Game/Big Game (Disc 2).cue"), b"x").unwrap();
        std::fs::write(ps1.join("Big Game/Big Game (Disc 1).bin"), b"x").unwrap();
        std::fs::write(
            ps1.join("Big Game/Big Game.m3u"),
            "Big Game (Disc 1).cue\nBig Game (Disc 2).cue\n",
        )
        .unwrap();
        std::fs::write(ps1.join("Small Game (Europe).chd"), b"xx").unwrap();
        std::fs::write(ps1.join("notes.txt"), b"x").unwrap();
        let roms = scan(&folder("ps1", &ps1, "duckstation"));
        let titles: Vec<&str> = roms.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Big Game", "Small Game"]);
        assert_eq!(roms[1].size, 2);
    }

    #[test]
    fn retroarch_needs_its_core_and_standalone_emulators_are_found() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("bin")).unwrap();
        let rom = root.path().join("snes/Game.sfc");
        let snes = folder("snes", &root.path().join("snes"), "retroarch");
        // Nothing installed: a clear message.
        let missing = command(&snes, &rom, &env(root.path())).unwrap_err();
        assert!(missing.user_message().contains("RetroArch"), "{missing}");

        std::fs::write(root.path().join("bin/retroarch"), b"").unwrap();
        let core_missing = command(&snes, &rom, &env(root.path())).unwrap_err();
        assert!(
            core_missing.user_message().contains("snes9x"),
            "{core_missing}"
        );
        let cores = root.path().join("home/.config/retroarch/cores");
        std::fs::create_dir_all(&cores).unwrap();
        std::fs::write(cores.join("snes9x_libretro.so"), b"").unwrap();
        let (program, args) = command(&snes, &rom, &env(root.path())).unwrap();
        assert_eq!(program, root.path().join("bin/retroarch"));
        assert_eq!(
            args,
            [
                "-f".to_owned(),
                "-L".to_owned(),
                cores
                    .join("snes9x_libretro.so")
                    .to_string_lossy()
                    .into_owned(),
                rom.to_string_lossy().into_owned()
            ]
        );

        std::fs::write(root.path().join("bin/dolphin-emu"), b"").unwrap();
        let gc = folder("gamecube", &root.path().join("gc"), "dolphin");
        let (program, args) = command(&gc, Path::new("/gc/Game.rvz"), &env(root.path())).unwrap();
        assert_eq!(program, root.path().join("bin/dolphin-emu"));
        assert_eq!(args, ["-b", "-e", "/gc/Game.rvz"]);
    }

    #[test]
    fn own_programs_and_folder_lookup() {
        let mut own = folder("psp", Path::new("/roms/psp"), CUSTOM);
        own.program = Some("/opt/emu/run".into());
        own.arguments = "--fast \"{rom}\"".into();
        let (program, args) = command(
            &own,
            Path::new("/roms/psp/My Game.iso"),
            &env(Path::new("/x")),
        )
        .unwrap();
        assert_eq!(program, PathBuf::from("/opt/emu/run"));
        assert_eq!(args, ["--fast", "/roms/psp/My Game.iso"]);

        let folders = [own.clone()];
        assert!(folder_of(&folders, Path::new("/roms/psp/a.iso")).is_some());
        assert!(folder_of(&folders, Path::new("/roms/pspx/a.iso")).is_none());
    }

    #[test]
    fn settings_are_checked() {
        let ok = EmulatorSettings {
            rom_folders: vec![folder("snes", Path::new("/r"), "retroarch")],
        };
        assert!(validate(&ok).is_ok());
        let wrong_emulator = EmulatorSettings {
            rom_folders: vec![folder("snes", Path::new("/r"), "dolphin")],
        };
        assert!(validate(&wrong_emulator).is_err());
        let mut custom = folder("snes", Path::new("/r"), CUSTOM);
        custom.program = Some("/bin/emu".into());
        assert!(validate(&EmulatorSettings {
            rom_folders: vec![custom.clone()]
        })
        .is_err());
        custom.arguments = "{rom}".into();
        assert!(validate(&EmulatorSettings {
            rom_folders: vec![custom]
        })
        .is_ok());
        let twice = EmulatorSettings {
            rom_folders: vec![
                folder("snes", Path::new("/r"), "retroarch"),
                folder("snes", Path::new("/s"), "retroarch"),
            ],
        };
        assert!(validate(&twice).is_err());
    }
}
