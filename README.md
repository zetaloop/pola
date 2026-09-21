# pola

pola is a native light and dark appearance tool for Windows 11 and macOS 27.

It follows system appearance changes, runs a weekly schedule with multiple transitions per day, provides a configurable global shortcut and launch-at-login control, and applies a wallpaper plus any configured commands for each mode. Settings use WinUI on Windows and AppKit on macOS.

Open pola to show the main window. A separate background process handles appearance changes, schedules, and the global shortcut while the window is closed. Appearance editors accept dropped wallpaper images and program files, with individual fields for command arguments. Schedule rules use native time pickers.

The default shortcut is `Ctrl+Shift+Alt+D`. Record a key combination in Settings to change it. Manual changes take effect immediately while future scheduled transitions continue normally.

## Build

Use the current Rust toolchain.

```sh
cargo build --release
```

On macOS, create the native app bundle with:

```sh
cargo build --profile MacOS --target-dir target/pola.app/Contents --config 'build.build-dir="target/app-build"'
```

The app is written to `target/pola.app`. On Windows, the release build stages the self-contained Windows App Runtime beside `target/release/pola.exe`.

On macOS, changing the system appearance uses System Events and may request Automation permission. Launch at login uses `~/Library/LaunchAgents/io.github.zetaloop.pola.plist`.

## Configuration

Settings are stored at:

```text
Windows  %LOCALAPPDATA%/pola/config.toml
macOS    ~/Library/Application Support/pola/config.toml
```

Editing saves the configuration. Appearance changes apply the wallpaper and commands. Commands are stored as an executable and an argument array, matching the process arguments exactly.
