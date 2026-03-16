import cadquery as cq
import math

# ============================================================
# Watch Case for Sellita SW280-1 Moonphase
# Patek Philippe / Vacheron Constantin inspired
# ============================================================

# --- Movement specs (Sellita DocTec SW280-1, drawing SW106933) ---
MVMT_FITTING_DIA = 25.60
MVMT_OVERALL_DIA = 26.00
MVMT_HEIGHT = 5.40
STEM_HEIGHT = 2.60          # stem center from caseback inner surface
STEM_DISTANCE = 12.80       # stem center to movement center

# --- Case dimensions ---
CASE_DIA = 39.0
CASE_R = CASE_DIA / 2.0
TOTAL_HEIGHT = 10.8
LUG_WIDTH = 20.0            # between lug arms (strap width)
LUG_TO_LUG = 47.0
SPRING_BAR_DIA = 1.8

# --- Derived ---
HALF_H = TOTAL_HEIGHT / 2.0
Z_BOT = -HALF_H             # caseback outer face
Z_TOP = HALF_H              # bezel top
CASEBACK_WALL = 1.0
BEZEL_RING_W = 2.5          # bezel lip width
DIAL_OPENING_DIA = 33.0     # generous dial window

# Movement cavity
MVMT_CLEARANCE = 0.10
MVMT_CAVITY_R = (MVMT_FITTING_DIA + MVMT_CLEARANCE) / 2.0
MVMT_POCKET_DEPTH = MVMT_HEIGHT + 0.4

# Crown
CROWN_TUBE_OD = 2.5
CROWN_TUBE_ID = 1.6

# ============================================================
# 1. CASE BODY - revolved profile (solid, then hollowed)
# ============================================================

# Profile in XZ plane. X = radial distance, Z = height.
# We draw the OUTER contour only, revolve to get solid,
# then cut the interior.

case_profile = (
    cq.Workplane("XZ")
    .moveTo(0, Z_BOT)
    # --- Bottom face (caseback) ---
    .lineTo(17.0, Z_BOT)
    .tangentArcPoint((2.0, 0.6), relative=True)
    # --- Lower flank: smooth swell to widest point ---
    .spline([
        (CASE_R - 0.3, -1.5),
        (CASE_R, 0.0),
    ], includeCurrent=True)
    # --- Upper flank: taper toward bezel ---
    .spline([
        (CASE_R - 0.2, 1.8),
        (CASE_R - 0.6, Z_TOP - 2.5),
    ], includeCurrent=True)
    # --- Bezel: gentle inward curve to the lip ---
    .spline([
        (CASE_R - 1.5, Z_TOP - 0.8),
        (DIAL_OPENING_DIA / 2.0 + BEZEL_RING_W, Z_TOP),
    ], includeCurrent=True)
    # --- Top face: flat ring (bezel lip) ---
    .lineTo(DIAL_OPENING_DIA / 2.0, Z_TOP)
    # --- Inner wall down to movement area ---
    .lineTo(DIAL_OPENING_DIA / 2.0, Z_TOP - 2.0)
    .lineTo(MVMT_CAVITY_R + 0.5, Z_TOP - 2.0)
    .lineTo(MVMT_CAVITY_R + 0.5, Z_BOT + CASEBACK_WALL)
    # --- Caseback inner floor ---
    .lineTo(0, Z_BOT + CASEBACK_WALL)
    .lineTo(0, Z_BOT)
    .close()
)

case_body = case_profile.revolve(360, (0, -10, 0), (0, 10, 0))

# ============================================================
# 2. MOVEMENT CAVITY - from caseback side
# ============================================================

mvmt_pocket = (
    cq.Workplane("XY")
    .workplane(offset=Z_BOT + CASEBACK_WALL)
    .circle(MVMT_CAVITY_R)
    .extrude(MVMT_POCKET_DEPTH)
)

# Rotor needs wider clearance (25.15mm + margin)
rotor_r = (25.15 + 0.30) / 2.0
rotor_pocket = (
    cq.Workplane("XY")
    .workplane(offset=Z_BOT + CASEBACK_WALL)
    .circle(rotor_r)
    .extrude(MVMT_POCKET_DEPTH + 1.2)
)

# ============================================================
# 3. CASEBACK RECESS
# ============================================================

caseback_recess = (
    cq.Workplane("XY")
    .workplane(offset=Z_BOT)
    .circle(36.0 / 2.0)
    .extrude(CASEBACK_WALL)
)

# ============================================================
# 4. STEM BORE at 3 o'clock
# ============================================================

stem_z = Z_BOT + CASEBACK_WALL + STEM_HEIGHT

# Crown tube bore (horizontal, along +X axis)
stem_bore = (
    cq.Workplane("XY")
    .workplane(offset=stem_z)
    .center(CASE_R, 0)
    .circle(CROWN_TUBE_OD / 2.0)
    .extrude(1, both=True)  # dummy - we'll use a real cylinder
)

# Proper horizontal bore through the case wall
stem_bore = (
    cq.Workplane("ZY")
    .workplane(offset=0)
    .center(stem_z, 0)
    .circle(CROWN_TUBE_OD / 2.0)
    .extrude(CASE_R + 3)
)

# ============================================================
# 5. LUGS - proper watch lugs with curved profile
#
# Each lug is built by lofting between a base section
# (where it meets the case) and a tip section (spring bar end).
# The lug curves downward from case to tip.
# ============================================================

lug_arm_w = 4.5             # single arm width
lug_arm_h_base = 5.5        # arm height at case junction
lug_arm_h_tip = 3.8         # arm height at spring bar end
lug_arm_spacing = LUG_WIDTH  # center-to-center between left and right arm

# The spring bar sits at the tip
spring_bar_y = LUG_TO_LUG / 2.0 - 2.5

# Z levels: lugs emerge from upper-mid case, curve down to mid-height at tip
lug_z_top_base = 1.5        # top of lug at case body
lug_z_top_tip = 0.5         # top of lug at tip (curves down)
lug_z_bot_base = lug_z_top_base - lug_arm_h_base
lug_z_bot_tip = lug_z_top_tip - lug_arm_h_tip


def make_single_lug(x_center, y_sign):
    """Build one lug arm using loft between base and tip cross-sections."""

    y_base = y_sign * (CASE_R - 2.0)  # where lug emerges from case
    y_tip = y_sign * (LUG_TO_LUG / 2.0)
    y_mid = y_sign * ((abs(y_base) + abs(y_tip)) / 2.0)

    # Base cross-section (at case body) - wider, taller
    base_wire = (
        cq.Workplane("XZ")
        .workplane(offset=y_base)
        .center(x_center, (lug_z_top_base + lug_z_bot_base) / 2.0)
        .rect(lug_arm_w, lug_arm_h_base)
    )

    # Mid cross-section
    mid_z_top = (lug_z_top_base + lug_z_top_tip) / 2.0
    mid_z_bot = (lug_z_bot_base + lug_z_bot_tip) / 2.0
    mid_h = mid_z_top - mid_z_bot
    mid_wire = (
        cq.Workplane("XZ")
        .workplane(offset=y_mid)
        .center(x_center, (mid_z_top + mid_z_bot) / 2.0)
        .rect(lug_arm_w - 0.3, mid_h)
    )

    # Tip cross-section (at spring bar) - narrower, shorter
    tip_wire = (
        cq.Workplane("XZ")
        .workplane(offset=y_tip)
        .center(x_center, (lug_z_top_tip + lug_z_bot_tip) / 2.0)
        .rect(lug_arm_w - 0.8, lug_arm_h_tip)
    )

    lug = (
        cq.Workplane("XZ")
        .workplane(offset=y_base)
        .center(x_center, (lug_z_top_base + lug_z_bot_base) / 2.0)
        .rect(lug_arm_w, lug_arm_h_base)
        .workplane(offset=y_mid - y_base)
        .center(x_center, (mid_z_top + mid_z_bot) / 2.0 - (lug_z_top_base + lug_z_bot_base) / 2.0)
        .rect(lug_arm_w - 0.3, mid_h)
        .loft(combine=True)
    )

    lug2 = (
        cq.Workplane("XZ")
        .workplane(offset=y_mid)
        .center(x_center, (mid_z_top + mid_z_bot) / 2.0)
        .rect(lug_arm_w - 0.3, mid_h)
        .workplane(offset=y_tip - y_mid)
        .center(x_center, (lug_z_top_tip + lug_z_bot_tip) / 2.0 - (mid_z_top + mid_z_bot) / 2.0)
        .rect(lug_arm_w - 0.8, lug_arm_h_tip)
        .loft(combine=True)
    )

    result = lug.union(lug2)

    # Fillet long edges for organic feel
    try:
        result = result.edges("|Y").fillet(1.2)
    except Exception:
        try:
            result = result.edges("|Y").fillet(0.8)
        except Exception:
            pass

    # Round the tip
    try:
        if y_sign > 0:
            result = result.edges(">Y").fillet(0.8)
        else:
            result = result.edges("<Y").fillet(0.8)
    except Exception:
        pass

    # Spring bar hole (horizontal, along X axis, through the tip)
    bar_y = y_sign * spring_bar_y
    bar_z = (lug_z_top_tip + lug_z_bot_tip) / 2.0
    bar_hole = (
        cq.Workplane("ZY")
        .workplane(offset=x_center)
        .center(bar_z, bar_y)
        .circle(SPRING_BAR_DIA / 2.0)
        .extrude(lug_arm_w + 2, both=True)
    )
    result = result.cut(bar_hole)

    return result


def make_lug_pair(y_sign):
    """Create left + right lug arms for 12 or 6 o'clock."""
    x_left = -(LUG_WIDTH / 2.0 - lug_arm_w / 2.0)
    x_right = (LUG_WIDTH / 2.0 - lug_arm_w / 2.0)
    left = make_single_lug(x_left, y_sign)
    right = make_single_lug(x_right, y_sign)
    return left.union(right)


lugs_12 = make_lug_pair(1)
lugs_6 = make_lug_pair(-1)

# ============================================================
# 6. COMBINE
# ============================================================

result = case_body
result = result.union(lugs_12).union(lugs_6)
result = result.cut(mvmt_pocket).cut(rotor_pocket)
result = result.cut(caseback_recess)
result = result.cut(stem_bore)

# ============================================================
# 7. FINAL FILLETS - subtle softening
# ============================================================

# Soften the sharp caseback edges
try:
    result = result.edges("<Z").fillet(0.25)
except Exception:
    pass

# ============================================================
# EXPORT
# ============================================================

cq.exporters.export(result, "/home/mcr/Projects/AI3D/sw280_watch_case.step")
cq.exporters.export(result, "/home/mcr/Projects/AI3D/sw280_watch_case.stl")

print("Done!")
print(f"  Case: {CASE_DIA}mm dia x {TOTAL_HEIGHT}mm tall")
print(f"  Dial opening: {DIAL_OPENING_DIA}mm (open, no glass)")
print(f"  Lugs: {LUG_TO_LUG}mm L2L, {LUG_WIDTH}mm strap width")
print(f"  Movement pocket: {MVMT_CAVITY_R*2:.1f}mm x {MVMT_POCKET_DEPTH:.1f}mm")
print(f"  Stem at Z={stem_z:.1f}mm from center")
