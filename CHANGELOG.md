# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/). Entries are generated with git-cliff from conventional
commits (see `cliff.toml`).

## [Unreleased]

### Added
- A log file, `logs/plotter-studio.<date>.log` in the data folder (seven days kept). Set
  `PLOTTER_STUDIO_LOG` to change the level, for example `debug`.

### Changed
- A library update that only raises a patch or minor version no longer puts a "version not found"
  warning on every older project; an older or a different major version still warns.

### Fixed
- A machine from an installed plugin's library, saved as the default, was not selected at
  start-up, so nothing could be prepared. It is chosen once the library loads, and a newer version
  of the chosen machine is picked up.
- With no machine chosen, or a job blocked by errors, the status bar, preview and Export tab now
  say why (and how many errors) instead of "Job out of date", "Nothing to preview yet" or an endless
  "Rendering…".
- Speed and force sliders end at what the machine can do, not at the tool's own limit.
- Start points are no longer drawn on shapes that have no operation.
- On Windows the executables carry the application icon (Explorer, shortcuts and the taskbar showed
  a generic one), the window shows it instead of the toolkit's default, and `plotter-studio.exe`
  no longer opens a command-line window that closes the app with it.

## [0.1.0] - 2026-11-22

### Added
- The Plotter Studio application: canvas editing, import of SVG and DXF drawings, the compiler,
  G-code export, a machine panel with a built-in simulator, autosave and crash recovery.
- The `pstudio` command line: `export`, `validate`, `inspect`, `devices` and `run`.
