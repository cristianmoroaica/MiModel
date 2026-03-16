//! Parse Claude's response to extract code blocks and text.

use crate::python::Engine;

#[derive(Debug, PartialEq)]
pub struct ParsedResponse {
    pub text: String,
    pub code: Option<CodeBlock>,
}

#[derive(Debug, PartialEq)]
pub struct CodeBlock {
    pub code: String,
    pub engine: Engine,
}

/// Extract TOML content from Claude's response (for Decompose phase).
/// Handles both raw TOML and ```toml fenced blocks.
/// Validates that the extracted content parses as TOML before returning.
pub fn parse_toml_response(response: &str) -> Result<String, String> {
    // 1. Try to extract from ```toml ... ``` fenced block
    let mut in_toml = false;
    let mut toml_content = String::new();

    for line in response.lines() {
        if !in_toml {
            if line.trim().starts_with("```toml") {
                in_toml = true;
                toml_content.clear();
                continue;
            }
        } else if line.trim() == "```" {
            // End of fenced block — validate and return
            match toml::from_str::<toml::Value>(&toml_content) {
                Ok(_) => return Ok(toml_content),
                Err(e) => return Err(format!("TOML in fenced block is invalid: {e}")),
            }
        } else {
            toml_content.push_str(line);
            toml_content.push('\n');
        }
    }

    // If we were in a toml block but never closed it, try what we have
    if in_toml && !toml_content.is_empty() {
        if toml::from_str::<toml::Value>(&toml_content).is_ok() {
            return Ok(toml_content);
        }
    }

    // 2. If no fenced block found, try the entire response as raw TOML
    match toml::from_str::<toml::Value>(response) {
        Ok(_) => Ok(response.to_string()),
        Err(_) => Err("No valid TOML found in response".to_string()),
    }
}

pub fn parse_response(response: &str) -> ParsedResponse {
    let mut text = String::new();
    let mut code_block: Option<CodeBlock> = None;
    let mut in_code = false;
    let mut current_lang = "";
    let mut code_content = String::new();

    for line in response.lines() {
        if !in_code {
            let trimmed = line.trim();
            if trimmed.starts_with("```cadquery")
                || trimmed.starts_with("```openscad")
                || trimmed.starts_with("```python")
            {
                in_code = true;
                current_lang = if trimmed.starts_with("```cadquery") {
                    "cadquery"
                } else if trimmed.starts_with("```openscad") {
                    "openscad"
                } else {
                    "python"
                };
                code_content.clear();
            } else if !text.is_empty() || !trimmed.is_empty() {
                text.push_str(line);
                text.push('\n');
            }
        } else if line.trim() == "```" {
            in_code = false;
            let engine = match current_lang {
                "cadquery" => Some(Engine::CadQuery),
                "openscad" => Some(Engine::OpenSCAD),
                "python" if code_content.contains("import cadquery") => Some(Engine::CadQuery),
                _ => None,
            };

            if let Some(eng) = engine {
                code_block = Some(CodeBlock { code: code_content.clone(), engine: eng });
            } else {
                text.push_str("```python\n");
                text.push_str(&code_content);
                text.push_str("```\n");
            }
        } else {
            code_content.push_str(line);
            code_content.push('\n');
        }
    }

    ParsedResponse { text: text.trim_end().to_string(), code: code_block }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cadquery_block() {
        let r = parse_response("Here's a box:\n\n```cadquery\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(10, 10, 10)\n```\n\n10mm cube.");
        assert!(r.text.contains("Here's a box:"));
        assert!(r.text.contains("10mm cube."));
        let code = r.code.unwrap();
        assert_eq!(code.engine, Engine::CadQuery);
        assert!(code.code.contains("cq.Workplane"));
    }

    #[test]
    fn test_parse_openscad_block() {
        let r = parse_response("```openscad\ncube([10, 10, 10]);\n```");
        let code = r.code.unwrap();
        assert_eq!(code.engine, Engine::OpenSCAD);
    }

    #[test]
    fn test_python_with_cadquery_import() {
        let r = parse_response("```python\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(5, 5, 5)\n```");
        assert_eq!(r.code.unwrap().engine, Engine::CadQuery);
    }

    #[test]
    fn test_python_without_cadquery_is_text() {
        let r = parse_response("Example:\n\n```python\nprint('hello')\n```");
        assert!(r.code.is_none());
        assert!(r.text.contains("print('hello')"));
    }

    #[test]
    fn test_plain_text_only() {
        let r = parse_response("What dimensions do you need?");
        assert!(r.code.is_none());
        assert_eq!(r.text, "What dimensions do you need?");
    }

    #[test]
    fn test_text_and_code() {
        let r = parse_response("Making it:\n\n```cadquery\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(10, 10, 10)\n```\n\nDone!");
        assert!(r.text.contains("Making it:"));
        assert!(r.text.contains("Done!"));
        assert!(r.code.is_some());
    }

    #[test]
    fn test_parse_toml_fenced() {
        let response = "Here are the components:\n\n```toml\n[[components]]\nid = \"body\"\n```\n\nLooks good.";
        let toml = parse_toml_response(response).unwrap();
        assert!(toml.contains("[[components]]"));
        assert!(toml.contains("id = \"body\""));
    }

    #[test]
    fn test_parse_toml_raw() {
        let response = "[[components]]\nid = \"body\"\nname = \"Body\"\n\n[assembly]\norder = [\"body\"]";
        let toml = parse_toml_response(response).unwrap();
        assert!(toml.contains("[[components]]"));
    }

    #[test]
    fn test_parse_toml_invalid() {
        let response = "I can't generate components for this.";
        assert!(parse_toml_response(response).is_err());
    }
}
