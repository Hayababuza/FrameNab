//! Core logic of FrameNab: project model, frame store, frame operations and undo history.
//!
//! This crate is deliberately independent of Tauri and any GUI so it stays fully testable.

pub mod archive;
pub mod error;
pub mod history;
pub mod image_ops;
pub mod input;
pub mod model;
pub mod ops;
pub mod recorder;
pub mod store;
pub mod transitions;

pub use error::{CoreError, Result};
pub use history::History;
pub use image_ops::{ImageOp, Transform};
pub use input::{Effects, InputEvents};
pub use model::*;
pub use ops::FrameOp;
pub use store::FrameStore;
pub use transitions::TransitionOp;
