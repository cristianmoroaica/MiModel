//! Phase-specific dispatch methods extracted from App.
//!
//! Handles per-phase send, response, input, and component lifecycle methods.
//! These methods remain on `impl App` but live in a separate file
//! to keep main.rs focused on struct definitions and the event loop.

use std::path::PathBuf;

use crate::claude_bridge::{self, BusyState};
use crate::phase::Phase;
use crate::python;
use crate::{parser, reference, reference_detect};

use super::*;

impl<'a> App<'a> {
    // -- Spec phase --

    pub(crate) fn send_spec_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        let ref_context = self.build_ref_context();

        let prompt = if self.claude.session_id.is_some() {
            if let Some(ref ctx) = ref_context {
                format!("[Reference context]\n{}\n\n{}", ctx, text)
            } else {
                text.to_string()
            }
        } else {
            text.to_string()
        };

        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            "spec", session_dir.as_deref()
        ).ok();
        self.claude.send_phase_prompt("spec", &prompt, &images, ref_context.as_deref(), mcp_config);
    }

    pub(crate) fn handle_spec_response(&mut self, response: &str) {
        // With MCP tools, code-block stripping and SPEC_COMPLETE detection are no longer
        // needed — structure is enforced by tool availability. This handler now just:
        // 1. Runs reference detection on Claude's freeform text
        // 2. Appends to the spec panel for visibility

        // Auto-detect external component references
        let known_slugs: Vec<String> = reference::load_library()
            .unwrap_or_default()
            .iter()
            .map(|(_, slug)| slug.clone())
            .collect();
        let detected = reference_detect::detect_references(response, &known_slugs);
        for det in &detected {
            if det.in_library {
                self.conversation.add("system",
                    &format!("Reference available: {} (use /ref {} to load)", det.name,
                        reference::slug_from_name(&det.name)));
            } else {
                self.conversation.add("system",
                    &format!("Detected component: {}. Use /ref {} to research and save.",
                        det.name, reference::slug_from_name(&det.name)));
            }
        }

        // Append Claude's text to the spec panel for visibility
        let mut spec_content = self.spec_panel.content().to_string();
        if !spec_content.is_empty() {
            spec_content.push_str("\n\n");
        }
        spec_content.push_str(response);
        self.spec_panel.set_content(&spec_content);
        self.right_panel.set_spec(&spec_content);

        // Persist the full spec narrative so it survives session reloads.
        // goal.md only captures structured fields; this preserves the full
        // design discussion (dimensions, rationale, context).
        if let Some(ref dir) = self.session.active_dir {
            let narrative_path = dir.join("spec_narrative.md");
            let _ = std::fs::write(&narrative_path, &spec_content);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn handle_spec_input(&mut self, _text: &str) {
        // Will be implemented in Chunk 6: send spec prompt, parse spec.toml response
    }

    // -- Decompose phase --

    pub(crate) fn send_decompose_prompt(&mut self, text: &str) {
        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            "decompose", session_dir.as_deref()
        ).ok();
        let ctx = self.build_phase_context();
        self.claude.send_phase_prompt("decompose", text, &[], ctx.as_deref(), mcp_config);
    }

    pub(crate) fn handle_decompose_response(&mut self, response: &str) {
        // With MCP tools, code-block stripping is no longer needed — the propose_component_tree
        // tool handles structured component proposals. This handler processes any freeform text
        // and also handles legacy TOML responses for backward compat during transition.

        match parser::parse_toml_response(response) {
            Ok(toml_str) => {
                self.parse_and_display_components(&toml_str);
                self.conversation.add("system",
                    "Component tree proposed. Type 'approve' to accept, or describe changes.");
            }
            Err(_) => {
                // No TOML — just conversation text, which is fine.
            }
        }
    }

    pub(crate) fn parse_and_display_components(&mut self, toml_str: &str) {
        use crate::tui::component_tree::TreeComponent;

        match toml::from_str::<toml::Value>(toml_str) {
            Ok(value) => {
                let mut tree_components = Vec::new();

                if let Some(components) = value.get("components").and_then(|c| c.as_array()) {
                    for comp in components {
                        let id = comp.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let name = comp.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
                        let assembly_op = comp.get("assembly_op").and_then(|v| v.as_str()).unwrap_or("none").to_string();
                        let depends_on: Vec<String> = comp.get("depends_on")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();

                        tree_components.push(TreeComponent { id, name, depends_on, assembly_op });
                    }
                }

                self.component_tree_panel.set_components(&tree_components);

                // Store the raw TOML for later merging into spec
                self.spec_panel.set_content(toml_str);
                self.right_panel.set_spec(toml_str);
            }
            Err(e) => {
                self.conversation.add("system", &format!("TOML parse error: {e}"));
            }
        }
    }

    pub(crate) fn approve_decomposition(&mut self) {
        let toml_str = self.spec_panel.content().to_string();
        if toml_str.is_empty() {
            self.conversation.add("system", "No component structure to approve. Ask Claude to decompose first.");
            return;
        }

        // TODO: Merge the component TOML into the spec.toml file
        // TODO: Create component directories using the parsed components
        // For now, just transition to Component phase

        self.conversation.add("system", "Component structure approved! Transitioning to Component phase.");
        self.phase = Phase::Build;
        self.layout_config.phase = Phase::Build;
        self.claude.session_id = None; // Fresh session for Component phase
        self.session.save(self.phase);
    }

    #[allow(dead_code)]
    pub(crate) fn handle_decompose_input(&mut self, _text: &str) {
        // Will be implemented in Chunk 6: send decompose prompt, parse component tree
    }

    // -- Build phase --

    pub(crate) fn send_build_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            "build", session_dir.as_deref()
        ).ok();
        let ctx = self.build_phase_context();
        self.claude.send_phase_prompt("build", text, &images, ctx.as_deref(), mcp_config);
    }

    // -- Refine phase --

    pub(crate) fn send_refine_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        let trimmed = text.trim().to_lowercase();

        if trimmed.starts_with("set ") {
            self.handle_param_edit(text);
            return;
        }
        if trimmed == "export" {
            self.handle_export();
            return;
        }

        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            "refine", session_dir.as_deref()
        ).ok();
        let ctx = self.build_phase_context();
        self.claude.send_phase_prompt("refine", text, &images, ctx.as_deref(), mcp_config);
    }

    pub(crate) fn handle_param_edit(&mut self, text: &str) {
        // Parse "set PARAM_NAME value" format
        let parts: Vec<&str> = text.trim().splitn(3, ' ').collect();
        if parts.len() < 3 {
            self.conversation.add("system", "Usage: set PARAM_NAME value (e.g., 'set OUTER_DIAMETER 42.0')");
            return;
        }

        let param_name = parts[1].to_uppercase();
        let value: f64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => {
                self.conversation.add("system", &format!("Invalid number: {}", parts[2]));
                return;
            }
        };

        self.conversation.add("system", &format!(
            "Parameter edit: {} = {} (zero-Claude rebuild)", param_name, value
        ));

        // In the future, this will:
        // 1. Write params JSON
        // 2. Call python::paramset()
        // 3. Rebuild assembly
        // 4. Update viewer
        // For now, just acknowledge the change
        self.conversation.add("system", "Parameter edit acknowledged. Full paramset integration pending PhaseSession wiring.");
    }


    pub(crate) fn handle_export(&mut self) {
        if let Some(ref session_dir) = self.session.active_dir {
            if let Some(stl_path) = self.session.latest_stl_path() {
                let export_stl = session_dir.join("export.stl");
                match std::fs::copy(&stl_path, &export_stl) {
                    Ok(_) => {
                        self.conversation.add("system", &format!("Exported to {}", export_stl.display()));
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Export failed: {e}"));
                    }
                }
            } else {
                self.conversation.add("system", "No model to export.");
            }
        } else {
            self.conversation.add("system", "No active session directory for export.");
        }
    }

    // -- Component phase --

    /// Start building the current component by sending an initial prompt to Claude.
    pub(crate) fn start_component_build(&mut self) {
        let idx = self.component_list.selected();
        let component_name = self.component_list.selected_id()
            .unwrap_or("unknown")
            .to_string();
        let total = self.component_list.len();
        let component_info = format!(
            "Generate CadQuery code for component {}/{}: '{}'.",
            idx + 1, total, component_name
        );
        self.send_component_prompt(&component_info, vec![]);
    }

    pub(crate) fn send_component_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            "component", session_dir.as_deref()
        ).ok();
        let ctx = self.build_phase_context();
        self.claude.send_phase_prompt("component", text, &images, ctx.as_deref(), mcp_config);
    }

    pub(crate) fn send_component_feedback(&mut self, text: &str, images: Vec<PathBuf>) {
        // Use "component" prompt for initial generation, "refinement" for feedback
        let phase_name = if self.session.current_code.is_some() {
            "refinement"
        } else {
            "component"
        };
        let session_dir = self.session.active_dir.clone();
        let mcp_config = claude_bridge::generate_mcp_config(
            phase_name, session_dir.as_deref()
        ).ok();
        let ctx = self.build_phase_context();
        self.claude.send_phase_prompt(phase_name, text, &images, ctx.as_deref(), mcp_config);
    }

    pub(crate) fn handle_component_build_result(&mut self, build_result: python::BuildResult, _code: String) {
        match build_result {
            python::BuildResult::Success(ref meta) => {
                self.conversation.add("system", &format!(
                    "Component built: {:.1} x {:.1} x {:.1} mm",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z
                ));

                // Update model panel
                let stl_path = self.session.latest_stl_path();
                let iteration = self.session.iteration();
                self.model_panel.update(meta, stl_path.as_deref(), iteration);
                let model_summary = format!(
                    "{:.1} x {:.1} x {:.1} mm\nIterations: {}\nEngine: {}\nWatertight: {}{}",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z,
                    iteration,
                    meta.engine.as_str(),
                    if meta.watertight { "yes" } else { "no" },
                    if meta.features.is_empty() { String::new() } else {
                        format!("\n\nFeatures:\n{}", meta.features.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"))
                    }
                );
                self.right_panel.set_model(&model_summary);

                // Update viewer
                if let Some(ref src) = stl_path {
                    let _ = self.viewer.update_working_stl(src);
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }

                self.conversation.add("system", "Type 'approve' to accept, or describe changes.");
                self.session.save(self.phase);
            }
            python::BuildResult::BuildError(ref e) | python::BuildResult::SyntaxError(ref e) => {
                self.conversation.add("system", &format!("Build error: {}", e.error));
            }
            python::BuildResult::Timeout => {
                self.conversation.add("system", "Build timed out.");
            }
        }
        self.claude.busy = BusyState::Idle;
    }

    pub(crate) fn approve_current_component(&mut self) {
        let total = self.component_list.len();
        let current = self.component_list.selected();

        if total == 0 {
            self.conversation.add("system", "No components to approve.");
            return;
        }

        // Trigger progressive assembly note if we have 2+ approved components
        // (current is 0-indexed, so current >= 1 means at least 2 approved)
        if current >= 1 {
            self.conversation.add("system", "Progressive assembly updated.");
        }

        if current + 1 < total {
            // Move to next component
            self.component_list.select_next();
            self.claude.session_id = None; // Fresh session for next component
            self.conversation.add("system", &format!(
                "Component approved! Moving to component {}/{}.",
                current + 2, total
            ));
            // Auto-start build for next component
            // self.start_component_build();
            self.session.save(self.phase);
        } else {
            // Last component — transition to Assembly
            self.conversation.add("system", "All components approved! Transitioning to Assembly phase.");
            self.phase = Phase::Build;
            self.layout_config.phase = Phase::Build;
            self.claude.session_id = None;
            self.session.save(self.phase);
        }
    }

    pub(crate) fn undo_component(&mut self) {
        if self.session.undo() {
            self.conversation.add("system", "Undid last component iteration.");
            // Update model panel and viewer
            if let Some(ref meta) = self.session.current_metadata {
                let stl_path = self.session.latest_stl_path();
                let iteration = self.session.iteration();
                self.model_panel.update(meta, stl_path.as_deref(), iteration);
                let model_summary = format!(
                    "{:.1} x {:.1} x {:.1} mm\nIterations: {}\nEngine: {}\nWatertight: {}{}",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z,
                    iteration,
                    meta.engine.as_str(),
                    if meta.watertight { "yes" } else { "no" },
                    if meta.features.is_empty() { String::new() } else {
                        format!("\n\nFeatures:\n{}", meta.features.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"))
                    }
                );
                self.right_panel.set_model(&model_summary);
                if let Some(ref src) = stl_path {
                    let _ = self.viewer.update_working_stl(src);
                }
            }
        } else {
            self.conversation.add("system", "Nothing to undo.");
        }
    }

    // -- Phase navigation --

    /// Attempt to switch to a different phase.
    /// For now, allows free navigation between phases.
    /// Prerequisite validation will be added when phase flows are implemented.
    pub(crate) fn try_switch_phase(&mut self, target: Phase) {
        if target == self.phase {
            return; // Already here
        }
        self.phase = target;
        self.layout_config.phase = target;
        // Force fresh Claude session so phase-specific system prompt and context
        // (spec conversation, goal.md, references) are re-injected. Without this,
        // --resume would continue the previous phase's session and silently drop
        // the new phase's context.
        self.claude.session_id = None;
        // Add system message about phase change
        self.conversation.add("system", &format!("Switched to {} phase", target.label()));
        self.session.save(self.phase);
    }
}
