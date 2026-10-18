//! Texture loading helpers.
//!
//! Native builds read `res/textures/<name>.png` at runtime. The wasm build has no filesystem,
//! so the `.cooked_tex` files produced by `PillLauncher -a assets` are embedded into the binary
//! and handed to the engine as raw bytes instead.

use pill_engine::game::*;

/// Builds a `ResourceLoader` for the texture whose file stem is `name`.
///
/// * Native: loads `textures/<name>.png` from the game's `res/` directory.
/// * Wasm: loads the embedded `textures/<name>.cooked_tex` bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn texture(name: &str) -> ResourceLoader {
    ResourceLoader::Path(format!("textures/{name}.png").into())
}

/// See the native variant: wasm embeds the cooked texture instead of touching the filesystem.
#[cfg(target_arch = "wasm32")]
pub fn texture(name: &str) -> ResourceLoader {
    let bytes: &[u8] = match name {
        "ship" => include_bytes!("../res/textures/ship.cooked_tex"),
        "foam_particle" => include_bytes!("../res/textures/foam_particle.cooked_tex"),
        "duck" => include_bytes!("../res/textures/duck.cooked_tex"),
        "container" => include_bytes!("../res/textures/container.cooked_tex"),
        other => panic!("texture \"{other}\" is not embedded for wasm; add it to assets.rs"),
    };
    ResourceLoader::Bytes(bytes.to_vec().into_boxed_slice())
}
