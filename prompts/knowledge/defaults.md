# Default Values for CadQuery Code Generation

Use these values unless the spec explicitly overrides them.

| Parameter | Default | Notes |
|-----------|---------|-------|
| Enclosure wall thickness | 2.0 mm | |
| Internal component clearance | 0.5 mm/side | Between component body and walls |
| Mating part clearance | 0.2 mm/side | Lids, covers, interlocking parts |
| Screw clearance hole | screw Ø + 0.4 mm | |
| Counterbore depth | = screw head height | Add 0.5 mm if head must be fully below surface |
| Port cutout clearance | 1.0 mm/side | |
| PCB standoff height | 5.0 mm | |
| Fillet radius (internal corners) | 0.5 mm | Stress relief |
| Fillet radius (external corners) | 1.0 mm | Comfort and durability |
| Rib thickness | 1.5 mm | 75% of 2.0 mm wall |
| Lip/tongue height | 2.0 mm | |
| Lip/tongue width | 1.2 mm | |
| Groove clearance/side | 0.1 mm | |
| Drain hole diameter | 3.5 mm | Required on all enclosed cavities |
| Debossed text depth | 0.6 mm | |
| Min text character height | 4.0 mm | |
| Alignment pin diameter | 3.0 mm | |
| Vent slot width | 2.0 mm | |
| Vent bar width | 2.0 mm | |

## Coordinate System
- +X = right, +Y = forward, +Z = up
- Origin at bottom-left-back corner of bounding box (CadQuery default)

## Code Conventions
- All dimensions as UPPERCASE constants at top of file
- Assign final solid to `result` variable
- Import CadQuery as `import cadquery as cq`
- Use parametric dimensions — never hardcode a number inline
