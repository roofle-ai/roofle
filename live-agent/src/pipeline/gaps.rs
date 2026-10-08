//! The closed vocabulary of gaps. The model classifies into this list; it is
//! never asked "is this important?", which small models answer badly.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GapType {
    PlanNoFallback,
    FuzzyCriteria,
    NoNumber,
    DismissedAlternative,
    NoOwnerOrDate,
    Contradiction,
}

impl GapType {
    /// Every type, in priority order (highest first).
    pub const ALL: [GapType; 6] = [
        GapType::PlanNoFallback,
        GapType::FuzzyCriteria,
        GapType::NoNumber,
        GapType::DismissedAlternative,
        GapType::NoOwnerOrDate,
        GapType::Contradiction,
    ];

    /// Wire name, as used in the JSON schemas and prompts.
    pub fn name(self) -> &'static str {
        match self {
            GapType::PlanNoFallback => "PLAN_NO_FALLBACK",
            GapType::FuzzyCriteria => "FUZZY_CRITERIA",
            GapType::NoNumber => "NO_NUMBER",
            GapType::DismissedAlternative => "DISMISSED_ALTERNATIVE",
            GapType::NoOwnerOrDate => "NO_OWNER_OR_DATE",
            GapType::Contradiction => "CONTRADICTION",
        }
    }

    /// Lower is more important.
    pub fn priority(self) -> usize {
        Self::ALL.iter().position(|g| *g == self).unwrap_or(usize::MAX)
    }

    /// The shape of question this gap type calls for (given to the writer).
    pub fn template(self) -> &'static str {
        match self {
            GapType::PlanNoFallback => "What if step Y fails? / What happens then?",
            GapType::FuzzyCriteria => "How will you know X is ready? / When would you call X done?",
            GapType::NoNumber => "Roughly how much / how many / how long?",
            GapType::DismissedAlternative => "Why not just Z? / Would Z not be enough?",
            GapType::NoOwnerOrDate => "Who's taking X on? / When do you need X by? (one, never both)",
            GapType::Contradiction => "Earlier you said A, now B. Which is it?",
        }
    }
}

/// The highest-priority item, so a chunk yields at most one question.
pub fn pick_best<T>(items: &[T], gap_type: impl Fn(&T) -> GapType) -> Option<&T> {
    items.iter().min_by_key(|i| gap_type(i).priority())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_priority_wins() {
        let gaps = [GapType::NoOwnerOrDate, GapType::PlanNoFallback, GapType::NoNumber];
        assert_eq!(pick_best(&gaps, |g| *g), Some(&GapType::PlanNoFallback));
        assert_eq!(pick_best::<GapType>(&[], |g| *g), None);
    }

    #[test]
    fn wire_names_round_trip() {
        for g in GapType::ALL {
            let json = serde_json::to_string(&g).unwrap();
            assert_eq!(json, format!("\"{}\"", g.name()));
        }
    }
}
