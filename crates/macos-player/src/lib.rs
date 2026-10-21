pub mod wire;

#[cfg(target_os = "macos")]
mod player;

#[cfg(target_os = "macos")]
pub use player::{authorization, probe, spawn, tokens};

#[cfg(target_os = "macos")]
#[used]
static LINKED_MARKER: &str = "bramley-linked:macos-player\0";

#[cfg(test)]
mod tests;
