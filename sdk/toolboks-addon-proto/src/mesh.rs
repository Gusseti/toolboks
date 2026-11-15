//! Cached meshes. Each tessellated primitive gets an id from a hash of its clip rectangle,
//! texture, vertices and indices. The add-on sends a primitive in full the first time and
//! as a bare id while it stays unchanged.
//!
//! Both sides keep the same cache by one rule: after a frame, the cache holds exactly the
//! ids that frame referenced. Nothing else is needed to keep them in step, and an id the
//! receiver does not hold is a protocol error ([`ProtocolError::UnknownMesh`]).
//! A 64-bit hash collision would draw a stale primitive; at a few hundred primitives per
//! frame the chance is negligible.

use crate::ProtocolError;
use egui::epaint::{Mesh, Primitive};
use egui::Rect;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MeshRef {
    New { id: u64, clip: Rect, mesh: Mesh },
    Cached { id: u64 },
}

impl MeshRef {
    pub fn id(&self) -> u64 {
        match self {
            Self::New { id, .. } | Self::Cached { id } => *id,
        }
    }
}

/// One drawable primitive: a mesh and its clip rectangle in view points.
#[derive(Clone, Debug, PartialEq)]
pub struct ClippedMesh {
    pub clip: Rect,
    pub mesh: Mesh,
}

pub fn mesh_id(clip: &Rect, mesh: &Mesh) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for value in [clip.min.x, clip.min.y, clip.max.x, clip.max.y] {
        hasher.write_u32(value.to_bits());
    }
    mesh.texture_id.hash(&mut hasher);
    hasher.write_usize(mesh.vertices.len());
    for vertex in &mesh.vertices {
        for value in [vertex.pos.x, vertex.pos.y, vertex.uv.x, vertex.uv.y] {
            hasher.write_u32(value.to_bits());
        }
        hasher.write(&vertex.color.to_array());
    }
    hasher.write_usize(mesh.indices.len());
    for index in &mesh.indices {
        hasher.write_u32(*index);
    }
    hasher.finish()
}

/// Add-on side.
#[derive(Default)]
pub struct MeshEncoder {
    known: HashSet<u64>,
}

impl MeshEncoder {
    /// Encodes one frame's primitives. Paint callbacks cannot cross the process boundary and
    /// are dropped.
    pub fn encode(
        &mut self,
        primitives: impl IntoIterator<Item = egui::ClippedPrimitive>,
    ) -> Vec<MeshRef> {
        let mut current = HashSet::new();
        let mut out = Vec::new();
        for primitive in primitives {
            let Primitive::Mesh(mesh) = primitive.primitive else {
                continue;
            };
            let clip = primitive.clip_rect;
            let id = mesh_id(&clip, &mesh);
            if self.known.contains(&id) || current.contains(&id) {
                out.push(MeshRef::Cached { id });
            } else {
                out.push(MeshRef::New { id, clip, mesh });
            }
            current.insert(id);
        }
        self.known = current;
        out
    }

    /// Forgets every id, so the next frame is sent in full (for a new connection).
    pub fn reset(&mut self) {
        self.known.clear();
    }
}

/// Host side.
#[derive(Default)]
pub struct MeshCache {
    meshes: HashMap<u64, Arc<ClippedMesh>>,
}

impl MeshCache {
    /// Resolves one frame's references into drawable meshes and updates the cache.
    /// On error the cache is left as it was.
    pub fn apply(&mut self, refs: Vec<MeshRef>) -> Result<Vec<Arc<ClippedMesh>>, ProtocolError> {
        let mut next: HashMap<u64, Arc<ClippedMesh>> = HashMap::with_capacity(refs.len());
        let mut out = Vec::with_capacity(refs.len());
        for reference in refs {
            let mesh = match reference {
                MeshRef::New { id, clip, mesh } => {
                    let mesh = Arc::new(ClippedMesh { clip, mesh });
                    next.insert(id, mesh.clone());
                    mesh
                }
                MeshRef::Cached { id } => {
                    let mesh = next
                        .get(&id)
                        .or_else(|| self.meshes.get(&id))
                        .cloned()
                        .ok_or(ProtocolError::UnknownMesh(id))?;
                    next.insert(id, mesh.clone());
                    mesh
                }
            };
            out.push(mesh);
        }
        self.meshes = next;
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode, FrameWriter};
    use egui::{pos2, ClippedPrimitive, Color32};

    fn rect_mesh(x: f32, color: Color32) -> ClippedPrimitive {
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(x, 0.0), pos2(x + 10.0, 10.0)),
            color,
        );
        ClippedPrimitive {
            clip_rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
            primitive: Primitive::Mesh(mesh),
        }
    }

    fn wire(refs: Vec<MeshRef>) -> Vec<MeshRef> {
        let mut writer = FrameWriter::new(Vec::new());
        writer.write(&refs).unwrap();
        decode(&writer.into_inner()[4..]).unwrap()
    }

    #[test]
    fn unchanged_primitives_travel_as_ids() {
        let mut encoder = MeshEncoder::default();
        let mut cache = MeshCache::default();
        let a = rect_mesh(0.0, Color32::RED);
        let b = rect_mesh(20.0, Color32::BLUE);

        let first = encoder.encode([a.clone(), b.clone(), a.clone()]);
        assert!(matches!(first[0], MeshRef::New { .. }));
        assert!(matches!(first[1], MeshRef::New { .. }));
        assert!(matches!(first[2], MeshRef::Cached { .. }));
        let drawn = cache.apply(wire(first)).unwrap();
        assert_eq!(drawn.len(), 3);
        assert_eq!(drawn[0], drawn[2]);
        assert_eq!(cache.len(), 2);

        let changed = rect_mesh(20.0, Color32::GREEN);
        let second = encoder.encode([a.clone(), changed.clone()]);
        assert!(matches!(second[0], MeshRef::Cached { .. }));
        assert!(matches!(second[1], MeshRef::New { .. }));
        let drawn = cache.apply(wire(second)).unwrap();
        let Primitive::Mesh(expected) = changed.primitive else {
            unreachable!()
        };
        assert_eq!(drawn[1].mesh, expected);
        assert_eq!(cache.len(), 2);

        // `b` was not in the last frame, so both sides dropped it.
        let third = encoder.encode([b]);
        assert!(matches!(third[0], MeshRef::New { .. }));
        cache.apply(third).unwrap();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn unknown_id_is_an_error_and_keeps_the_cache() {
        let mut encoder = MeshEncoder::default();
        let mut cache = MeshCache::default();
        cache
            .apply(encoder.encode([rect_mesh(0.0, Color32::RED)]))
            .unwrap();
        let error = cache.apply(vec![MeshRef::Cached { id: 42 }]).unwrap_err();
        assert!(matches!(error, ProtocolError::UnknownMesh(42)));
        assert_eq!(cache.len(), 1);
    }
}
