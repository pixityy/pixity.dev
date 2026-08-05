# pixity desktop

The [pixity.dev](https://pixity.dev) desktop, as a native app for macOS, Windows
and Linux. Built with [Tauri](https://tauri.app).

The window loads the live site, so the app is the shell around it rather than a
copy of it: notifications, an unread badge on the dock, the microphone
permission declared once, and (on macOS and Linux) screen capture, which WebKit
has never implemented in a webview.

## Build

Needs [Rust](https://rustup.rs) and the Tauri prerequisites for your platform.

```sh
cargo install tauri-cli --version "^2" --locked
cargo tauri icon icon-source.png
cargo tauri dev
cargo tauri build
```

`cargo tauri build` writes installers to `target/release/bundle/`. Which ones
depends on the machine you build on: `.exe` and `.msi` on Windows, `.dmg` on
macOS, `.deb`/`.rpm`/`.AppImage` on Linux. There is no cross-compiling.

## Licence

GPL-3.0.
