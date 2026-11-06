//! Kernlogik von FrameNab: Projektmodell, Frame-Speicher, Frame-Operationen und Undo-Verlauf.
//!
//! Diese Crate ist bewusst unabhängig von Tauri und GUI, damit sie vollständig testbar bleibt.

pub mod error;
pub mod history;
pub mod model;
pub mod ops;
pub mod store;

pub use error::{CoreError, Result};
pub use history::History;
pub use model::*;
pub use store::FrameStore;
