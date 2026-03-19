# Resin Printing (SLA/DLP) Constraints

## Dimensional Accuracy
- Tolerance: +/- 0.1 mm typical, budget +/- 0.15 mm on mating surfaces
- Minimum feature size: 0.5 mm practical minimum (0.2 mm absolute)
- Minimum hole diameter: 1.0 mm (smaller holes close during cure)
- Minimum gap between features: 1.0 mm (prevents UV blooming/fusion)
- Minimum gap between walls: 0.5 mm (IPA wash access)

## Wall Thickness

| Feature | Minimum | Recommended |
|---------|---------|-------------|
| Supported wall (2+ edges) | 0.6 mm | 1.0 mm |
| Unsupported wall (free-standing) | 1.0 mm | 1.5 mm |
| Enclosure outer wall | 1.5 mm | 2.0–2.5 mm |
| Hollow section wall | 2.0 mm | 2.5 mm |
| Bar between vent slots | 1.5 mm | 2.0 mm |
| Pin diameter | 1.5 mm | 2.0 mm+ |

## Overhangs and Orientation
- Unsupported overhang length: 1.0 mm max without supports
- Safe overhang angle from horizontal: 30° minimum (19° absolute)
- Horizontal bridge span: 21 mm max before sagging
- Orient flat surfaces 30–45° to build plate to reduce peel forces

## Warping Prevention
- Max unsupported flat panel: 40 mm before ribs needed
- Add ribs every 20–30 mm on panels > 40 mm in any dimension
- Rib thickness: 60–80% of adjacent wall (e.g., 1.5 mm for 2.0 mm wall)
- Rib height: up to 3× wall thickness

## Drain Holes (Hollow Parts)
- Diameter: 3.5 mm minimum
- Quantity: 2 holes on opposite sides preferred
- Required on ANY enclosed cavity — uncured resin causes cracking or outgassing

## Material Considerations
- Standard resin: brittle (2–5% elongation) — avoid snap-fits
- Tough 2000 resin: 79% elongation — suitable for snap-fits and press-fits
- Durable resin: 55% elongation — good for functional parts
- Post-cure shrinkage: account for 0.1–0.2% dimensional change
