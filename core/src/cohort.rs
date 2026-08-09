//! Which characters the application is actually about.
//!
//! Every character on the account is synced — `If-Modified-Since` makes
//! twenty-three of them cost almost nothing, since a character who has not
//! logged out since the last sync answers `304` with no body. But sync is not
//! enrolment.
//!
//! Enrolment is explicit and per character. The cohort is what the run is
//! measured against, what the interface shows, and what every "which of my
//! characters should do this" answer ranges over. Everyone else stays in the
//! database for exactly one purpose: explaining why something is already owned.
//! When a mount cannot be collected again because Aeltor looted it in 2016,
//! Aeltor has to still be there to say so — without cluttering a view that is
//! about Somechar.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::character::{Character, CharacterKey, Roster};

/// The enrolled characters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cohort {
    members: BTreeSet<CharacterKey>,
}

impl Cohort {
    pub fn new() -> Self {
        Cohort::default()
    }

    pub fn contains(&self, key: &CharacterKey) -> bool {
        self.members.contains(key)
    }

    pub fn enrol(&mut self, key: CharacterKey) {
        self.members.insert(key);
    }

    pub fn withdraw(&mut self, key: &CharacterKey) {
        self.members.remove(key);
    }

    /// Enrol or withdraw, and report what the state became.
    pub fn toggle(&mut self, key: &CharacterKey) -> bool {
        if self.contains(key) {
            self.withdraw(key);
            false
        } else {
            self.enrol(key.clone());
            true
        }
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &CharacterKey> {
        self.members.iter()
    }

    /// The enrolled characters, in roster order.
    pub fn members<'r>(&self, roster: &'r Roster) -> Vec<&'r Character> {
        roster
            .characters
            .iter()
            .filter(|character| self.contains(&character.key))
            .collect()
    }

    /// The rest of the account: not shown, not measured, and kept only to
    /// explain why something is already spent.
    pub fn bystanders<'r>(&self, roster: &'r Roster) -> Vec<&'r Character> {
        roster
            .characters
            .iter()
            .filter(|character| !self.contains(&character.key))
            .collect()
    }

    /// Drop anyone who is no longer on the account.
    ///
    /// A deleted or transferred character would otherwise sit in the cohort
    /// forever, quietly keeping goals settled that nothing can any longer
    /// account for.
    pub fn prune(&mut self, roster: &Roster) {
        self.members.retain(|key| roster.get(key).is_some());
    }
}

impl From<Vec<CharacterKey>> for Cohort {
    fn from(keys: Vec<CharacterKey>) -> Self {
        Cohort {
            members: keys.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Faction;

    fn character(realm: &str, name: &str) -> Character {
        Character {
            key: CharacterKey::new(realm, name),
            id: 1,
            realm_id: 2,
            display_name: name.to_string(),
            realm_name: realm.to_string(),
            level: 80,
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            wow_account_id: 1,
        }
    }

    fn roster() -> Roster {
        Roster::new(vec![
            character("emerald-dream", "Somechar"),
            character("mannoroth", "Aeltor"),
            character("dalaran", "Moodivh"),
        ])
    }

    #[test]
    fn enrolment_is_opt_in_so_a_new_cohort_is_empty() {
        // Twenty-three characters syncing does not mean twenty-three characters
        // showing.
        assert!(Cohort::new().is_empty());
        assert_eq!(Cohort::new().members(&roster()).len(), 0);
    }

    #[test]
    fn everyone_not_enrolled_is_kept_as_a_bystander() {
        // They are why something is already owned. Losing them would lose the
        // explanation.
        let mut cohort = Cohort::new();
        cohort.enrol(CharacterKey::new("emerald-dream", "Somechar"));

        let roster = roster();
        assert_eq!(cohort.members(&roster).len(), 1);
        assert_eq!(cohort.bystanders(&roster).len(), 2);
    }

    #[test]
    fn members_come_back_in_roster_order_not_enrolment_order() {
        let mut cohort = Cohort::new();
        cohort.enrol(CharacterKey::new("mannoroth", "Aeltor"));
        cohort.enrol(CharacterKey::new("dalaran", "Moodivh"));

        let roster = roster();
        let names: Vec<&str> = cohort
            .members(&roster)
            .iter()
            .map(|c| c.display_name.as_str())
            .collect();
        assert_eq!(names, ["Moodivh", "Aeltor"]);
    }

    #[test]
    fn toggling_reports_what_it_became() {
        let mut cohort = Cohort::new();
        let key = CharacterKey::new("emerald-dream", "Somechar");
        assert!(cohort.toggle(&key));
        assert!(cohort.contains(&key));
        assert!(!cohort.toggle(&key));
        assert!(!cohort.contains(&key));
    }

    #[test]
    fn pruning_drops_characters_who_have_left_the_account() {
        // A transferred character left in the cohort would go on settling goals
        // that nothing can account for.
        let mut cohort = Cohort::from(vec![
            CharacterKey::new("emerald-dream", "Somechar"),
            CharacterKey::new("gone", "Ghost"),
        ]);
        cohort.prune(&roster());
        assert_eq!(cohort.len(), 1);
        assert!(cohort.contains(&CharacterKey::new("emerald-dream", "Somechar")));
    }
}
