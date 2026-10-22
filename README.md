# MetaboKit2

MetaboKit's DDA and DIA desktop application, alongside the original command-line
tools and visualizers.

## Repository layout

- `metabokit-gui/`: the current Tauri 2 desktop app, its DDA and DIA processing
  engines, web UI, icons, tests, and macOS packaging script. This is an independent
  Cargo workspace imported from `Metabokit-DDA`, including its current local changes.
- `mdda/`: the original DDA CLI, R plotting script, and `mdda-viz` visualizer.
- `mdia/`: the original DIA CLI and `mdia-viz` visualizer.

The GUI uses the engines in its own `crates/` directory; it does not require either
original CLI to be built or the previous repository to be present.

## Run the desktop app

Install Rust 1.88 or newer and the native development tools required by Tauri 2
(Xcode Command Line Tools on macOS). The GUI serves its local HTML/CSS/JavaScript
directly, so it needs no frontend package installation or dev server.

```sh
cd metabokit-gui
cargo run --locked -p metabokit-app
```

Run the Rust tests from the same directory:

```sh
cargo test --locked --workspace
```

The optional UI checks use Deno and Node.js with Playwright, respectively:

```sh
deno test ui/dia.test.js
node scripts/ui-smoke.cjs
```

The smoke script uses installed Google Chrome by default. Set
`METABOKIT_BROWSER_CHANNEL` to select another Playwright browser channel.

## macOS packaging

Install the Tauri CLI once:

```sh
cargo install tauri-cli --version '^2' --locked
```

From `metabokit-gui/`, build a local unsigned DMG:

```sh
./build-macos.sh
```

Packaging requires full Xcode. See
[MACOS_DISTRIBUTION.md](metabokit-gui/MACOS_DISTRIBUTION.md) for signed, notarized,
and universal builds, and [DIA.md](metabokit-gui/DIA.md) for DIA processing and
integration details.

Build output, generated Tauri files, dependencies, local datasets, environment
files, and signing credentials are excluded by the root `.gitignore`. Keep
application lockfiles, icons, source files, and license notices in Git.
