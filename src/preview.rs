//! Terminal 3D preview using braille characters.
#![allow(dead_code)]

use crate::stl::{StlMesh, Vec3};

const BRAILLE_BASE: u32 = 0x2800;

fn dot_bit(col: usize, row: usize) -> u8 {
    match (col, row) {
        (0, 0) => 0, (0, 1) => 1, (0, 2) => 2,
        (1, 0) => 3, (1, 1) => 4, (1, 2) => 5,
        (0, 3) => 6, (1, 3) => 7,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ViewAngle {
    Front, Back, Right, Left, Top, Bottom,
}

impl ViewAngle {
    pub fn project(&self, p: &Vec3) -> (f32, f32) {
        match self {
            ViewAngle::Front => (p.x, p.z),
            ViewAngle::Back => (-p.x, p.z),
            ViewAngle::Right => (p.y, p.z),
            ViewAngle::Left => (-p.y, p.z),
            ViewAngle::Top => (p.x, p.y),
            ViewAngle::Bottom => (p.x, -p.y),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ViewAngle::Front => "front", ViewAngle::Back => "back",
            ViewAngle::Right => "right", ViewAngle::Left => "left",
            ViewAngle::Top => "top", ViewAngle::Bottom => "bottom",
        }
    }

    pub fn next(&self) -> ViewAngle {
        match self {
            ViewAngle::Front => ViewAngle::Right,
            ViewAngle::Right => ViewAngle::Back,
            ViewAngle::Back => ViewAngle::Left,
            ViewAngle::Left => ViewAngle::Top,
            ViewAngle::Top => ViewAngle::Bottom,
            ViewAngle::Bottom => ViewAngle::Front,
        }
    }

    pub fn prev(&self) -> ViewAngle {
        match self {
            ViewAngle::Front => ViewAngle::Bottom,
            ViewAngle::Right => ViewAngle::Front,
            ViewAngle::Back => ViewAngle::Right,
            ViewAngle::Left => ViewAngle::Back,
            ViewAngle::Top => ViewAngle::Left,
            ViewAngle::Bottom => ViewAngle::Top,
        }
    }
}

pub fn render_braille(mesh: &StlMesh, view: ViewAngle, term_width: usize) -> String {
    let char_cols = term_width.min(80);
    let char_rows = char_cols / 2;
    let dot_cols = char_cols * 2;
    let dot_rows = char_rows * 4;

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for tri in &mesh.triangles {
        for v in &tri.vertices {
            let (px, py) = view.project(v);
            min_x = min_x.min(px); max_x = max_x.max(px);
            min_y = min_y.min(py); max_y = max_y.max(py);
        }
    }

    let range_x = (max_x - min_x).max(0.001);
    let range_y = (max_y - min_y).max(0.001);
    let margin = 2.0;
    let scale = ((dot_cols as f32 - margin * 2.0) / range_x)
        .min((dot_rows as f32 - margin * 2.0) / range_y);

    let mut dots = vec![vec![false; dot_cols]; dot_rows];

    for tri in &mesh.triangles {
        for edge in [(0, 1), (1, 2), (2, 0)] {
            let (ax, ay) = view.project(&tri.vertices[edge.0]);
            let (bx, by) = view.project(&tri.vertices[edge.1]);
            let x0 = ((ax - min_x) * scale + margin) as i32;
            let y0 = ((ay - min_y) * scale + margin) as i32;
            let x1 = ((bx - min_x) * scale + margin) as i32;
            let y1 = ((by - min_y) * scale + margin) as i32;
            draw_line(&mut dots, x0, y0, x1, y1, dot_cols, dot_rows);
        }
    }

    let mut output = String::new();
    for row in (0..dot_rows).step_by(4).rev() {
        for col in (0..dot_cols).step_by(2) {
            let mut bits: u8 = 0;
            for dr in 0..4 {
                for dc in 0..2 {
                    let r = row + dr;
                    let c = col + dc;
                    if r < dot_rows && c < dot_cols && dots[r][c] {
                        bits |= 1 << dot_bit(dc, dr);
                    }
                }
            }
            output.push(char::from_u32(BRAILLE_BASE + bits as u32).unwrap_or(' '));
        }
        output.push('\n');
    }
    output
}

fn draw_line(
    dots: &mut [Vec<bool>], x0: i32, y0: i32, x1: i32, y1: i32,
    width: usize, height: usize,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);

    loop {
        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            dots[y as usize][x as usize] = true;
        }
        if x == x1 && y == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stl::{StlMesh, Triangle, Vec3};

    fn make_test_mesh() -> StlMesh {
        StlMesh {
            triangles: vec![Triangle {
                vertices: [
                    Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                    Vec3 { x: 10.0, y: 0.0, z: 0.0 },
                    Vec3 { x: 5.0, y: 0.0, z: 10.0 },
                ],
            }],
            min: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            max: Vec3 { x: 10.0, y: 0.0, z: 10.0 },
        }
    }

    #[test]
    fn test_render_produces_braille() {
        let mesh = make_test_mesh();
        let output = render_braille(&mesh, ViewAngle::Front, 40);
        assert!(!output.is_empty());
        assert!(output.chars().any(|c| (0x2800..=0x28FF).contains(&(c as u32))));
    }

    #[test]
    fn test_view_rotation_cycle() {
        assert!(matches!(ViewAngle::Front.next(), ViewAngle::Right));
        assert!(matches!(ViewAngle::Right.next(), ViewAngle::Back));
        assert!(matches!(ViewAngle::Front.prev(), ViewAngle::Bottom));
    }
}
