//! Binary STL reader for terminal preview rendering.

use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone)]
pub struct Triangle {
    pub vertices: [Vec3; 3],
}

#[derive(Debug)]
pub struct StlMesh {
    pub triangles: Vec<Triangle>,
    pub min: Vec3,
    pub max: Vec3,
}

impl StlMesh {
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        if data.len() < 84 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "STL too short"));
        }

        let tri_count = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
        let expected = 84 + tri_count * 50;
        if data.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("STL truncated: expected {expected} bytes, got {}", data.len()),
            ));
        }

        let mut triangles = Vec::with_capacity(tri_count);
        let mut min = Vec3 { x: f32::MAX, y: f32::MAX, z: f32::MAX };
        let mut max = Vec3 { x: f32::MIN, y: f32::MIN, z: f32::MIN };

        for i in 0..tri_count {
            let offset = 84 + i * 50;
            let mut verts = [Vec3 { x: 0.0, y: 0.0, z: 0.0 }; 3];
            for v in 0..3 {
                let vo = offset + 12 + v * 12;
                let x = f32::from_le_bytes([data[vo], data[vo+1], data[vo+2], data[vo+3]]);
                let y = f32::from_le_bytes([data[vo+4], data[vo+5], data[vo+6], data[vo+7]]);
                let z = f32::from_le_bytes([data[vo+8], data[vo+9], data[vo+10], data[vo+11]]);
                verts[v] = Vec3 { x, y, z };
                min.x = min.x.min(x); min.y = min.y.min(y); min.z = min.z.min(z);
                max.x = max.x.max(x); max.y = max.y.max(y); max.z = max.z.max(z);
            }
            triangles.push(Triangle { vertices: verts });
        }

        Ok(StlMesh { triangles, min, max })
    }

    pub fn extents(&self) -> Vec3 {
        Vec3 { x: self.max.x - self.min.x, y: self.max.y - self.min.y, z: self.max.z - self.min.z }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_triangle_stl(v0: [f32; 3], v1: [f32; 3], v2: [f32; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 84 + 50];
        data[80] = 1; // 1 triangle
        for (i, coord) in v0.iter().chain(v1.iter()).chain(v2.iter()).enumerate() {
            let bytes = coord.to_le_bytes();
            let off = 96 + i * 4;
            data[off..off + 4].copy_from_slice(&bytes);
        }
        data
    }

    #[test]
    fn test_parse_single_triangle() {
        let data = make_triangle_stl([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 10.0, 0.0]);
        let mesh = StlMesh::from_bytes(&data).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        assert!((mesh.extents().x - 10.0).abs() < 0.001);
        assert!((mesh.extents().y - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_bounding_box() {
        let data = make_triangle_stl([-5.0, -3.0, 0.0], [5.0, 3.0, 0.0], [0.0, 0.0, 7.0]);
        let mesh = StlMesh::from_bytes(&data).unwrap();
        assert!((mesh.min.x - (-5.0)).abs() < 0.001);
        assert!((mesh.max.x - 5.0).abs() < 0.001);
        assert!((mesh.extents().z - 7.0).abs() < 0.001);
    }

    #[test]
    fn test_reject_truncated() {
        assert!(StlMesh::from_bytes(&vec![0u8; 50]).is_err());
    }
}
