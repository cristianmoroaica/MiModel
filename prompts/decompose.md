You are decomposing a 3D model specification into manufacturable components.

Input: a TOML specification (provided below).

Rules:
- Each component must be independently buildable in CadQuery
- Aim for components under 80 lines of CadQuery (guideline, not hard limit)
- Define dependencies: which components must exist before others
- No circular dependencies
- Specify assembly operations: fuse, subtract, or none
- Each component is modeled at the origin; assembly applies transforms

Output ONLY a TOML fragment with [[components]] and [assembly] sections.
No prose, no explanation, no code blocks — just raw TOML.

Format for each component:
[[components]]
id = "snake_case_name"
name = "Human Name"
description = "What this component is and does"
depends_on = ["other_id"]
assembly_op = "fuse|subtract|none"
assembly_target = "component_id_or_empty"

[components.parameters]
param_name = { value = 0.0, unit = "mm", description = "..." }

[components.constraints]
items = ["constraint text"]

End with:
[assembly]
order = ["id1", "id2"]
notes = "Assembly notes"
