# Toolboks app icon

`logo.png` (project root) is the source image. `app-icon.png` is the 256 px copy the window loads at
start-up (window, taskbar, Alt+Tab). `app.ico` holds 16 to 256 px sizes and is embedded in
toolboks.exe by `build.rs`, so Explorer and pinned taskbar buttons show it too.
To change the logo, replace `logo.png` and regenerate both files from it.
