# Wavey

A local two-player boat-battling playground built with [Pill Engine](https://pillengine.org). Ride procedural waves, ram the other ship, drop bombs and spawn some ducks and crates. Features buoyancy physics, collision effects, controller rumble and live physics tweaking.

## Getting Started

1. Install [Rust](https://rustup.rs/) and clone this repository (including the `no_miners` branch).
2. From the repository root, run:

   `cargo run --manifest-path engine/pill_launcher/Cargo.toml -- -a run -p examples/wavey`

   **Note:** The current `engine/Cargo.toml` contains a developer-specific absolute path to the Wavey crate; update it for your checkout until that is fixed.

## Controls

| Action | Keyboard (Player 1) | Gamepad (Players 1–2) |
| --- | --- | --- |
| Move left / right | `A` / `D` | Left stick |
| Charge dive / release | Hold `S` / release | `A` (press; hold support TODO) |
| Drop bomb | `Space` | `X` |
| Horn | Left `Ctrl` | `Y` |
| Spawn crate | `O` | — |
| Spawn duck | `P` | — |
| Camera zoom | `U` / `I` | — |

The in-game UI exposes live water, ship physics and visual-effect settings. Ship impacts can trigger collision effects and gamepad rumble.

## Showcase

*Gameplay GIF or screenshot coming soon.*

## TODO

- [ ] Add a damage indicator UI and numbers floating
- [ ] Decide when a ship is ded and how to indicate that
- [ ] You can shoot someone so fast that they die instantly - anime like
- [ ] Destroying crates when you run over them with high force
- [ ] Decide about the meta of the game - what are the conditions for winning etc
- [ ] How to carry the crates -> do we change the sprite or do we just load them on the barges
- [ ] Fix gamepad `A` hold-to-charge diving (currently uses a just-pressed input).
- [ ] Tune buoyancy, mass and colliders separately for ships, ducks and crates.
- [ ] Investigate FPS drops when spawning lots of crates.
- [ ] Improve camera framing when the two ships drift apart.
- [ ] Polish collision damage, impact effects and rumble balancing.
- [ ] Add an objective / scoring system and explore online multiplayer.
- [ ] Remove hard-coded local workspace paths and update the game title/config.
- [ ] Record a proper gameplay showcase.

## License

See the repository's [MIT](../../LICENSE-MIT) and [Apache 2.0](../../LICENSE-APACHE) license files.
