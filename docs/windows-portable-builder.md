# Portable Hookshot Builder on Windows

Put the release's `HookshotBuilder.exe` setup in a writable folder on your SSD, then run:

```powershell
.\HookshotBuilder.exe --portable
```

The setup creates a marked `HookshotBuilder` data folder beside itself. Keep this folder: it contains the installed
Builder (the Flutter app and its Rust engine), downloaded update parts, settings, the work folder, DLSS SDK downloads
you opt into, and the default build files. The normal setup continues to use `%LOCALAPPDATA%\HookshotBuilder`;
portable mode does not import that installation or its settings. To reuse existing game files, select them explicitly
in the new Builder.

The default game folder is `Hookshot` beside the data folder. Keep the game and Builder on the same volume. You can
move their containing folder or change that volume's drive letter: the game records a relative association to its
Builder (`.hookshot_builder_home`), and paths selected on the same volume are saved relative to the Builder home in
`builder.ini`. Paths on another volume stay absolute and may need selecting again after moving. Desktop and Start menu
shortcuts are not part of portable mode; this fork's setup does not create them.

## The portable layout

```text
SSD\
  HookshotBuilder.exe          the setup you downloaded (run it with --portable)
  HookshotBuilder\            the marked home (keep it)
    .hookshot-portable        the marker: "Hookshot Builder portable home v1"
    builder.ini               settings; same-volume paths are relative
    downloads\                checked update parts (removed after install)
    versions\
      current.txt             the installed tag, a relative name
      <tag>\
        HookshotBuilder\      the Flutter app, its data folder and the Rust engine
          HookshotBuilder.exe
          tools\              the tracked build sources and play profile
  Hookshot\                   the default game folder
    hookshot.exe              the setup launcher, copied by the build
    .hookshot_builder_home    "../HookshotBuilder", relative to this game
    hookshot_version.txt      version=<tag> and builder_local=1
```

The app finds the home without an absolute path: it walks up from its own executable (up to three parents) to the
folder marked `.hookshot-portable`, and checks `HookshotBuilder` beside the setup for the older ZIP layout. Opening the
installed `HookshotBuilder.exe` directly therefore works after a move, with or without the setup's environment.
The setup passes an absolute `HOOKSHOT_SETUP_BASE` and `HOOKSHOT_BUILDER_PORTABLE=1` to the app and the build children
for that process tree only. No user/system environment variables or registry configuration are installed.

The Rust packager copies the packaged setup beside the game as `hookshot.exe` (Python's `BESIDE[0]` rule), so the game
folder's launcher and update offer keep working after a move. The earlier `portable.txt` beside the executable is still
accepted: it keeps settings under `<exe folder>\data`. A marked home is preferred when both exist. On Linux the same
marker and `portable.txt` rules apply to the app's own data, but the Windows setup is the only part that creates a
marked home; the Linux game launcher's update path remains the AppImage/XDG one.

## Updates and repair

The original setup can be opened again without the flag while its marked data folder is beside it. Updates, repair
and the setup's fast launch use that same home. Unchanged parts are copied from the installed version into staging,
so the installed Builder stays usable until publication and enough free space for both versions is needed. A repair
publishes its complete staged directory before discarding the replaced one. If Windows blocks restoration after a
failed rename, the preserved old directory remains beside it as `<version>.rollback-*`; close programs using these
folders before recovering it.

The game launcher (`<game>\hookshot.exe`) uses the game-relative `.hookshot_builder_home` when
`hookshot_version.txt` says `builder_local=1`; it never follows a stored absolute Builder path in that mode. A missing or invalid `.hookshot_builder_home` stops portable Builder discovery rather than silently using another
Builder. The game itself never checks for updates: started directly it starts at once and logs where updates come from.
Rebuilding a portable game with a standard Builder removes the association after successful output finalization, so the
launcher uses the standard Builder again.

Portable mode requires the packaged setup launcher; a source checkout without that launcher cannot produce a
relocatable portable game's update association. For an incomplete installed version, use **Repair** through the
packaged setup launcher, keeping the portable home in place.

## Limits

- A portable root must have the valid marker, be a dedicated ordinary drive/UNC directory, and fit the native setup's
  Windows path limits. Do not use the current directory, device namespace paths or a volume root as a substitute.
- Physical removable-drive relocation, a complete public-release/game repair and live update downloads were not tested
  in this fork. The game-free tests cover relative resolution, moved-folder detection, update metadata without
  absolute paths and the setup/package wiring.
- Windows, .NET and third-party tools can still create their own temporary files and operating-system caches; this is
  not a zero-write or sandbox mode.
- Never share a built game folder, extracted files, saves, caches or downloaded SDK packages. The contribution rules
  in [CONTRIBUTING](../CONTRIBUTING.md) apply equally to portable installations.
