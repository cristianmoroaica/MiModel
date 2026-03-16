use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Spec,
    Decompose,
    Component,
    Assembly,
    Refinement,
}

impl Phase {
    pub fn index(self) -> usize {
        match self {
            Phase::Spec => 0,
            Phase::Decompose => 1,
            Phase::Component => 2,
            Phase::Assembly => 3,
            Phase::Refinement => 4,
        }
    }

    pub fn from_index(i: usize) -> Option<Phase> {
        match i {
            0 => Some(Phase::Spec),
            1 => Some(Phase::Decompose),
            2 => Some(Phase::Component),
            3 => Some(Phase::Assembly),
            4 => Some(Phase::Refinement),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Spec => "Spec",
            Phase::Decompose => "Decompose",
            Phase::Component => "Component",
            Phase::Assembly => "Assembly",
            Phase::Refinement => "Refinement",
        }
    }

    /// Returns true only if `target` is exactly one step forward.
    pub fn can_advance_to(self, target: Phase) -> bool {
        target.index() == self.index() + 1
    }

    /// Returns true only if `target` is exactly one step back.
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
        assert_eq!(Phase::Decompose.index(), 1);
        assert_eq!(Phase::Component.index(), 2);
        assert_eq!(Phase::Assembly.index(), 3);
        assert_eq!(Phase::Refinement.index(), 4);
    }

    #[test]
    fn test_can_advance() {
        assert!(Phase::Spec.can_advance_to(Phase::Decompose));
        assert!(Phase::Decompose.can_advance_to(Phase::Component));
        assert!(!Phase::Spec.can_advance_to(Phase::Component));
    }

    #[test]
    fn test_can_go_back() {
        assert!(Phase::Decompose.can_go_back_to(Phase::Spec));
        assert!(Phase::Component.can_go_back_to(Phase::Decompose));
        assert!(Phase::Assembly.can_go_back_to(Phase::Component));
    }

    #[test]
    fn test_label() {
        assert_eq!(Phase::Spec.label(), "Spec");
        assert_eq!(Phase::Component.label(), "Component");
    }
}
