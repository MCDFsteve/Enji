//! The engine of Enji's shell. It switches the optional modules and keeps
//! the choice in `modules.json`, so the app starts with the modules it
//! had.

use bern::CommandError;
use enji_kit::store;
use enji_shell_protocol::shell::{self, State};

pub struct ShellEngine {
    state: State,
}

impl Default for ShellEngine {
    fn default() -> Self {
        Self { state: State {} }
    }
}

impl shell::Commands for ShellEngine {
    fn set_module(&mut self, id: String, on: bool) -> Result<(), CommandError> {
        bern::modules::set_enabled(&id, on);
        Ok(())
    }
}

impl shell::Engine for ShellEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn initial_modules(&self) -> Option<Vec<String>> {
        let kept = store::read("modules.json");
        let running = kept["running"].as_array()?;
        Some(running.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
    }

    fn modules_changed(&mut self, enabled: &[String]) {
        store::write("modules.json", &serde_json::json!({ "running": enabled }));
    }
}
