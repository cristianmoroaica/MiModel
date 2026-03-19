# Spatial Layout — Two-Pass Build Methodology

Multi-component builds use a two-pass approach to prevent collisions and ensure
everything fits before investing time in detailed geometry.

## Pass 1: Rough Placeholders

Before building ANY component in detail, create rough placeholders for ALL components.

Each placeholder MUST include:
- Correct overall bounding box (from spec/references)
- Mounting holes and bosses at correct positions
- Major pockets and cutouts at correct sizes
- Cable/connector openings

Each placeholder MUST NOT include:
- Fillets, chamfers, or rounds
- Surface details, labels, or debossed text
- Ribs, gussets, or stiffening features
- Vent patterns
- Aesthetic features

Write placeholders to `components/<id>/code.py` using the standard code rules.
Keep them simple — 20-40 lines of CadQuery maximum.

## Layout Assembly

After ALL placeholders are built, write `assembly/layout.py`:

1. Import all placeholder STEPs
2. Position each at its final location (translate/rotate)
3. Apply boolean operations (cut, union, intersect)
4. Verify:
   - No overlapping volumes between components
   - Adequate clearances (0.5 mm minimum between bodies)
   - All components fit within the overall envelope
   - Mounting interfaces align (holes line up across components)
5. Run screenshot_viewer to visually confirm spatial arrangement

The layout assembly is the spatial truth. Save it — it becomes the reference for
the detail pass.

## Pass 2: Detailed Refinement

After the layout assembly is verified, refine each component:

1. Read `assembly/layout.py` to understand the spatial arrangement
2. Read neighboring components' `code.py` for interface dimensions
3. Refine the placeholder code — add full detail WITHIN the same bounding envelope:
   - Keep the same overall dimensions and mounting interface
   - Add fillets, chamfers, ribs, vents, labels
   - Add internal features (cable channels, alignment pins)
4. If bounding box must change, update layout assembly and re-verify

CRITICAL: The placeholder establishes the spatial contract. Detail pass adds
complexity without changing the shape's footprint or mounting interface.

## When to Use

- Any build with 2+ components
- Single-component builds with 3+ referenced sub-components (e.g., an enclosure
  housing a motor, driver board, and power supply)
- Any build where the spec mentions multiple parts that must fit together
