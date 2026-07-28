#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod error;
pub mod game;
pub mod input;
pub mod net;

pub use error::RedixelError;
pub use game::{Game, GameContext, InputBind, InputQuery};
pub use input::{InputAction, InputSource, KeyCode, KeyState, MouseButton};
pub use net::{ClientId, NetworkChannel, NetworkEvent, NetworkManager, NoOpNetwork, SERVER_ID, SequenceBuffer};
