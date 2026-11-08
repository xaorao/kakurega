//! How the workspace is shown, as chosen by the owner. Every field only changes how things are presented: none of them can touch
//! approval, the model route, policy, what leaves the computer or updates. `from_env` is what the workspace did before there was a
//! configuration file, so a program that does not pass settings behaves exactly as it always did.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// What rings the bell or shows a desktop notification: `None` is automatic, `Some("bell")` the bell only, `Some("off")` nothing.
    /// Any other text is read as automatic, the way `KAKU_NOTIFY` always was.
    pub notify: Option<String>,
    /// Whether colour is used.
    pub color: bool,
    /// Whether to start with only the end of a review shown, as `/quiet` does.
    pub quiet: bool,
    /// Said once at the top of the workspace: for example that a configuration file was not applied, and why.
    pub notice: Option<String>,
}

impl Settings {
    pub fn from_env() -> Settings {
        Settings {
            notify: std::env::var("KAKU_NOTIFY").ok(),
            color: std::env::var_os("NO_COLOR").is_none(),
            quiet: false,
            notice: None,
        }
    }
}
