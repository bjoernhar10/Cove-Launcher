# Flatpak and Flathub

Cove's Flatpak repackages the release `.deb` of each version (the same build as on GitHub) on
the GNOME 51 runtime. GNOME 51 is the last GNOME runtime with webkit2gtk-4.1, which Tauri 2
needs on Linux; it is supported until about September 2027. Before then Cove has to move to a
Tauri release with GTK 4 support or build webkit2gtk-4.1 itself.

Files: `packaging/flatpak/io.github.bjoernhar10.Cove_Launcher.yml` (manifest) and
`io.github.bjoernhar10.Cove_Launcher.metainfo.xml` (AppStream data for the store page).

## How Cove behaves inside Flatpak

`src-tauri/src/flatpak.rs` (`flatpak::active()` = `/.flatpak-info` exists):

- Games Cove starts itself (native, Proton through umu-launcher, GOG and Epic games Cove
  installed) run inside the sandbox, with the i386 compat and GL32 extensions for 32-bit games.
- Host programs run on the host through `flatpak-spawn --host`: Steam (system or Flatpak),
  emulators found in the host's `PATH` (seen under `/run/host`) or installed as Flatpak, and
  RetroArch cores from the host's system folders. Game options (MangoHud, gamescope …) don't
  apply to them.
- Games a store client starts (Steam) run on the host; their playtime is followed through the
  host's `/proc` (`flatpak::host_processes`).
- MangoHud and gamescope come from the Flathub extensions
  `org.freedesktop.Platform.VulkanLayer.MangoHud` and `…VulkanLayer.gamescope` (users install
  them). GameMode is not in the Flatpak.
- The self-updater is off; Flatpak updates Cove.

## Build locally

```sh
flatpak install flathub org.flatpak.Builder org.gnome.Platform//51 org.gnome.Sdk//51
cd packaging/flatpak
git clone --depth 1 https://github.com/flathub/shared-modules.git
flatpak run org.flatpak.Builder --user --install --force-clean build-dir io.github.bjoernhar10.Cove_Launcher.yml
flatpak run io.github.bjoernhar10.Cove_Launcher
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.bjoernhar10.Cove_Launcher.yml
```

The Flatpak workflow (`.github/workflows/flatpak.yml`) builds `Cove-x86_64.flatpak` and
`Cove-aarch64.flatpak` for every published release and attaches them to it.

## Submit to Flathub (once)

1. Fork https://github.com/flathub/flathub and create a branch from `new-pr`.
2. Add `io.github.bjoernhar10.Cove_Launcher.yml`, the metainfo file and `flathub.json`
   (`{ "only-arches": ["x86_64", "aarch64"] }`), and `shared-modules` as a git submodule:
   `git submodule add https://github.com/flathub/shared-modules.git`.
3. Open a pull request against `new-pr` titled "Add io.github.bjoernhar10.Cove_Launcher" and
   answer the reviewers. They may ask about the repackaged `.deb` and the
   `org.freedesktop.Flatpak` permission (it starts Steam and emulators on the host).
4. After the merge Flathub creates `flathub/io.github.bjoernhar10.Cove_Launcher`. Its
   external data checker (`x-checker-data`) opens a pull request for every new GitHub release.

## Every release

Add a `<release>` entry with the version, date and a short description of what's new to the
metainfo file (also in the Flathub repository) and update the `.deb` URLs and checksums in the
manifest here (the Flathub bot does it there).
