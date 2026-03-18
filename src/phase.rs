use serde::{Serialize, Deserialize, Deserializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Phase {
    Spec,
    Build,
    Refine,
}

// Custom deserializer for backward compatibility with old session.json files
// that used Decompose/Component/Assembly/Refinement
impl<'de> Deserialize<'de> for Phase {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "Spec" => Ok(Phase::Spec),
            "Build" | "Decompose" | "Component" | "Assembly" => Ok(Phase::Build),
            "Refine" | "Refinement" => Ok(Phase::Refine),
            other => Err(serde::de::Error::unknown_variant(other, &["Spec", "Build", "Refine"])),
        }
    }
}

impl Phase {
    pub fn index(self) -> usize {
        match self {
            Phase::Spec => 0,
            Phase::Build => 1,
            Phase::Refine => 2,
        }
    }

    pub fn from_index(i: usize) -> Option<Phase> {
        match i {
            0 => Some(Phase::Spec),
            1 => Some(Phase::Build),
            2 => Some(Phase::Refine),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Spec => "Spec",
            Phase::Build => "Build",
            Phase::Refine => "Refine",
        }
    }

    pub fn can_advance_to(self, target: Phase) -> bool {
        target.index() == self.index() + 1
    }

    pub fn can_go_back_to(self, target: Phase) -> bool {
        self.index() > 0 && target.index() + 1 == self.index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_ordering() {
        assert_eq!(Phase::Spec.index(), 0);
        assert_eq!(Phase::Build.index(), 1);
        assert_eq!(Phase::Refine.index(), 2);
    }

    #[test]
    fn test_can_advance() {
        assert!(Phase::Spec.can_advance_to(Phase::Build));
        assert!(Phase::Build.can_advance_to(Phase::Refine));
        assert!(!Phase::Spec.can_advance_to(Phase::Refine));
    }

    #[test]
    fn test_can_go_back() {
        assert!(Phase::Build.can_go_back_to(Phase::Spec));
        assert!(Phase::Refine.can_go_back_to(Phase::Build));
    }

    #[test]
    fn test_label() {
        assert_eq!(Phase::Spec.label(), "Spec");
        assert_eq!(Phase::Build.label(), "Build");
        assert_eq!(Phase::Refine.label(), "Refine");
    }

    #[test]
    fn test_backward_compat_deserialize() {
        // Old phase names should map to new ones
        let p: Phase = serde_json::from_str("\"Component\"").unwrap();
        assert_eq!(p, Phase::Build);
        let p: Phase = serde_json::from_str("\"Refinement\"").unwrap();
        assert_eq!(p, Phase::Refine);
        let p: Phase = serde_json::from_str("\"Decompose\"").unwrap();
        assert_eq!(p, Phase::Build);
        let p: Phase = serde_json::from_str("\"Assembly\"").unwrap();
        assert_eq!(p, Phase::Build);
    }
}
