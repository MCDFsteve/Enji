//! The protocol of Enji's shell, which assembles the modules: the core
//! (`enji`), always running, and For You, Charts and Search, which can be
//! switched off.

#[bern::protocol(namespace = "shell", version = 1)]
pub mod shell {
    use bern::prelude::*;

    #[state]
    pub struct State {}

    #[commands]
    pub trait Commands {
        /// Switch an optional module on or off: `recommend` (For You),
        /// `charts` or `search`.
        fn set_module(&mut self, id: String, on: bool) -> Result<(), CommandError>;
    }
}
