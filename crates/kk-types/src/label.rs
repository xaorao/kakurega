use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Integrity {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label {
    pub level: Level,
    pub compartments: BTreeSet<String>,
    pub integrity: Integrity,
    pub sensitivity_unknown: bool,
}

impl Label {
    pub fn new(level: Level) -> Self {
        Label {
            level,
            compartments: BTreeSet::new(),
            integrity: Integrity::Trusted,
            sensitivity_unknown: false,
        }
    }

    pub fn bottom() -> Self {
        Label::new(Level::Public)
    }

    pub fn with_compartment(mut self, name: &str) -> Self {
        self.compartments.insert(name.to_string());
        self
    }

    pub fn untrusted(mut self) -> Self {
        self.integrity = Integrity::Untrusted;
        self
    }

    pub fn unknown_sensitivity(mut self) -> Self {
        self.sensitivity_unknown = true;
        self
    }

    pub fn effective_level(&self) -> Level {
        if self.sensitivity_unknown {
            self.level.max(Level::Restricted)
        } else {
            self.level
        }
    }

    pub fn join(&self, other: &Label) -> Label {
        Label {
            level: self.level.max(other.level),
            compartments: self
                .compartments
                .union(&other.compartments)
                .cloned()
                .collect(),
            integrity: self.integrity.max(other.integrity),
            sensitivity_unknown: self.sensitivity_unknown || other.sensitivity_unknown,
        }
    }

    pub fn confidentiality_within(&self, clearance: &Label) -> bool {
        self.effective_level() <= clearance.effective_level()
            && self.compartments.is_subset(&clearance.compartments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn level() -> impl Strategy<Value = Level> {
        prop_oneof![
            Just(Level::Public),
            Just(Level::Internal),
            Just(Level::Confidential),
            Just(Level::Restricted)
        ]
    }

    fn label() -> impl Strategy<Value = Label> {
        (
            level(),
            proptest::collection::btree_set("[a-c]{1,2}", 0..4),
            prop_oneof![Just(Integrity::Trusted), Just(Integrity::Untrusted)],
            any::<bool>(),
        )
            .prop_map(
                |(level, compartments, integrity, sensitivity_unknown)| Label {
                    level,
                    compartments,
                    integrity,
                    sensitivity_unknown,
                },
            )
    }

    proptest! {
        #[test]
        fn join_is_commutative(a in label(), b in label()) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn join_is_associative(a in label(), b in label(), c in label()) {
            prop_assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
        }

        #[test]
        fn join_is_idempotent(a in label()) {
            prop_assert_eq!(a.join(&a), a);
        }

        #[test]
        fn bottom_is_identity(a in label()) {
            prop_assert_eq!(a.join(&Label::bottom()), a);
        }

        #[test]
        fn join_never_lowers_any_component(a in label(), b in label()) {
            let j = a.join(&b);
            prop_assert!(j.level >= a.level && j.level >= b.level);
            prop_assert!(a.compartments.is_subset(&j.compartments) && b.compartments.is_subset(&j.compartments));
            prop_assert!(j.integrity >= a.integrity && j.integrity >= b.integrity);
            prop_assert!(j.sensitivity_unknown || !(a.sensitivity_unknown || b.sensitivity_unknown));
            prop_assert!(j.effective_level() >= a.effective_level());
        }

        #[test]
        fn low_integrity_never_lowers_confidentiality(a in label(), b in label()) {
            let hostile = Label { integrity: Integrity::Untrusted, ..b.clone() };
            let j = a.join(&hostile);
            prop_assert!(j.level >= a.level);
            prop_assert!(a.compartments.is_subset(&j.compartments));
        }
    }

    #[test]
    fn unknown_sensitivity_is_treated_as_restricted_for_clearance() {
        let unknown = Label::new(Level::Public).unknown_sensitivity();
        let clearance = Label::new(Level::Confidential);
        assert!(!unknown.confidentiality_within(&clearance));
        assert!(unknown.confidentiality_within(&Label::new(Level::Restricted)));
    }

    #[test]
    fn compartments_must_be_covered_by_the_clearance() {
        let item = Label::new(Level::Confidential).with_compartment("payroll");
        assert!(!item.confidentiality_within(&Label::new(Level::Restricted)));
        assert!(
            item.confidentiality_within(&Label::new(Level::Restricted).with_compartment("payroll"))
        );
    }
}
