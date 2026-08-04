/// An opaque handle to a texture the engine has been asked to load.
///
/// Issued the moment a game calls `load_texture`, before any decoding or GPU
/// upload: the renderer sits behind the runtime's frame loop and a headless
/// server has no GPU at all. Drawing with a handle whose texture failed to
/// decode is not an error; it renders as the missing-texture checkerboard.
///
/// Only meaningful to the engine instance that issued it — never serialise one
/// or send it over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(u32);

impl TextureId {
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}
