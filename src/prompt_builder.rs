//! Prompt builder — constructs phase-specific user messages for Claude.
//!
//! Each phase in the pipeline (spec, decompose, component, assembly, refinement)
//! sends Claude a different system prompt and a structured user message.
//! This module handles both: locating the right system prompt file and
//! building the user message for each phase.

/// Locate `prompts/<phase_name>.md`.
///
/// Walks up from cwd and from the binary directory, looking for a
/// `prompts/` directory that contains `<phase_name>.md`.  This mirrors
/// the logic used by `find_system_prompt` in `claude.rs` but accepts any
/// phase filename.
pub fn load_phase_system_prompt(phase_name: &str) -> Result<String, String> {
    let filename = format!("{phase_name}.md");

    let starts: Vec<std::path::PathBuf> = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ]
    .into_iter()
    .flatten()
    .collect();

    for start in &starts {
        let mut dir = start.as_path();
        loop {
            let candidate = dir.join("prompts").join(&filename);
            if candidate.exists() {
                return std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("Failed to read {}: {e}", candidate.display()));
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    Err(format!(
        "prompts/{filename} not found. Run from within the MiModel project."
    ))
}

/// Build the user message for the **spec** phase.
///
/// Passes the latest question from Claude and the user's answer so that
/// Claude can record the answer as a key-value pair and ask the next
/// question.
pub fn build_spec_prompt(question: &str, user_answer: &str) -> String {
    format!(
        "Question: {question}\n\nUser answer: {user_answer}"
    )
}

/// Build the user message for the **decompose** phase.
///
/// The full TOML specification is embedded so Claude can produce the
/// component breakdown.
pub fn build_decompose_prompt(spec_toml: &str) -> String {
    format!(
        "Specification (TOML):\n\n```toml\n{spec_toml}\n```"
    )
}

/// Build the user message for the **component** phase.
///
/// - `id` — snake_case component identifier
/// - `params` — tuples of (name, value, unit)
/// - `constraints` — free-text constraint strings
/// - `dep_code` — optional CadQuery source of dependency components
///   (provided for reference; Claude must not import them)
pub fn build_component_prompt(
    id: &str,
    params: &[(String, String, String)],
    constraints: &[String],
    dep_code: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!("Component: {id}"));

    if !params.is_empty() {
        let param_lines: Vec<String> = params
            .iter()
            .map(|(name, value, unit)| format!("  {name} = {value} {unit}"))
            .collect();
        parts.push(format!("Parameters:\n{}", param_lines.join("\n")));
    }

    if !constraints.is_empty() {
        let constraint_lines: Vec<String> = constraints
            .iter()
            .map(|c| format!("  - {c}"))
            .collect();
        parts.push(format!("Constraints:\n{}", constraint_lines.join("\n")));
    }

    if let Some(code) = dep_code {
        parts.push(format!(
            "Dependency component code (reference only — do not import):\n\n```cadquery\n{code}\n```"
        ));
    }

    parts.join("\n\n")
}

/// Build the user message for the **refinement** phase.
///
/// - `code` — current CadQuery source for the component
/// - `feedback` — user's natural-language change request
/// - `params` — current parameter set (name, value, unit)
pub fn build_refinement_prompt(
    code: &str,
    feedback: &str,
    params: &[(String, String, String)],
) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!(
        "Current component code:\n\n```cadquery\n{code}\n```"
    ));

    parts.push(format!("User feedback: {feedback}"));

    if !params.is_empty() {
        let param_lines: Vec<String> = params
            .iter()
            .map(|(name, value, unit)| format!("  {name} = {value} {unit}"))
            .collect();
        parts.push(format!("Current parameters:\n{}", param_lines.join("\n")));
    }

    parts.join("\n\n")
}

/// Build the user message for the **assembly** phase.
///
/// - `components` — list of `(id, cadquery_code, assembly_op, transform_notes)`
/// - `notes` — overall assembly notes from the decompose step
pub fn build_assembly_prompt(
    components: &[(String, String, String, String)],
    notes: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!("Assembly notes: {notes}"));

    for (id, code, op, transform) in components {
        parts.push(format!(
            "Component `{id}` (op: {op}, transform: {transform}):\n\n```cadquery\n{code}\n```"
        ));
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_spec_prompt() {
        let prompt = build_spec_prompt("What is this model for?", "A watch case");
        assert!(prompt.contains("watch case"));
    }

    #[test]
    fn test_build_component_prompt() {
        let params = vec![("DIAMETER".to_string(), "25.6".to_string(), "mm".to_string())];
        let constraints = vec!["Must fit SW280 movement".to_string()];
        let prompt = build_component_prompt("movement_cavity", &params, &constraints, None);
        assert!(prompt.contains("movement_cavity"));
        assert!(prompt.contains("DIAMETER"));
        assert!(prompt.contains("25.6"));
    }

    #[test]
    fn test_build_refinement_prompt() {
        let prompt = build_refinement_prompt(
            "current code here",
            "Make it 2mm wider",
            &[("WIDTH".to_string(), "10.0".to_string(), "mm".to_string())],
        );
        assert!(prompt.contains("current code here"));
        assert!(prompt.contains("2mm wider"));
    }

    #[test]
    fn test_load_system_prompt() {
        let prompt = load_phase_system_prompt("spec");
        assert!(prompt.is_ok());
        assert!(prompt.unwrap().contains("ONE question at a time"));
    }
}
