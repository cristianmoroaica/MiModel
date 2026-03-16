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
}
