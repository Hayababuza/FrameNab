#[cfg(windows)]
pub mod gdi;
#[cfg(target_os = "linux")]
pub mod portal;
#[cfg(target_os = "linux")]
pub mod screencast;
pub mod xcap;
