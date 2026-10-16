mod config;
mod ctx;
mod fade;
mod fullscreen;
mod run;
mod scene;
#[cfg(test)]
mod tests;

pub use config::{Config, RenderMode};
pub use ctx::Ctx;
pub use run::run;
pub use scene::{Scene, Trans};

use scene::*;
