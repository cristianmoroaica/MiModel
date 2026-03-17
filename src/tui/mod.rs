pub mod layout;
pub mod input_bar;
pub mod conversation;
pub mod project_tree;
pub mod model_panel;
pub mod spec_panel;
pub mod component_tree;
pub mod component_list;
pub mod param_editor;
pub mod status_bar;

/// Focus state — which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Input,
    ProjectTree,
    Conversation,
    RightPanel,
}

/// Results from background threads.
pub enum BackgroundResult {
    ClaudeResponse {
        result: Result<String, String>,
        session_id: Option<String>,
    },
    BuildComplete(crate::python::BuildResult),
    ReferenceResearch {
        name: String,
        result: Result<String, String>,
    },
}

