# pola

Automatically applies a Windows appearance profile when the system switches between light and dark.

## Install

```powershell
scoop bucket add zetaloop https://github.com/zetaloop/zetaloop
scoop install pola
```

Pola starts after installation and at sign-in. Uninstall it with `scoop uninstall pola`.

## Configuration

Edit `<Scoop root>/persist/pola/data/config.toml`:

```toml
[light]
wallpaper = "Wallpapers/Light.jpg"
commands = [
    { program = "wallpaper64.exe", args = ["-control", "openProfile", "-profile", "Light"] },
]

[dark]
wallpaper = "Wallpapers/Dark.jpg"
commands = [
    { program = "wallpaper64.exe", args = ["-control", "openProfile", "-profile", "Dark"] },
]
```

Wallpaper and command paths are relative to the `data` directory. Commands are started in order without waiting for them to exit. Wallpapers span all displays.

Errors are written to `data/pola.log`.
