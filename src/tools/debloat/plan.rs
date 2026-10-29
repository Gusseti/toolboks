//! The change engine: reads the state of every item, counts what a selection would do, applies it
//! with recorded previous values, and undoes it.
#![allow(dead_code)]

use super::backend::Registry;
use super::model::{Applicable, Edition, Env, Hive, Item, RegValue, Risk, State, Status};

/// What the Review panel shows for the ticked items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Review {
    /// Ticked items that would actually change something (state `Active`, applicable).
    pub changes: usize,
    pub safe: usize,
    pub careful: usize,
    /// Among `changes`: how many need admin rights that are missing.
    pub needs_admin: usize,
    /// Ticked items that cannot be applied (edition not supported) or have nothing to do.
    pub skipped: usize,
}

/// One registry value as it was before a change, so it can be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoOp {
    pub id: &'static str,
    pub hive: Hive,
    pub path: &'static str,
    pub name: &'static str,
    /// `None` means the value did not exist.
    pub previous: Option<RegValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed(String),
    /// Written, but reading it back did not show the expected value (policy or edition ignores it).
    NotEffective,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub id: &'static str,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub applied: Vec<Applied>,
    pub undo: Vec<UndoOp>,
}

fn state_of(item: &Item, reg: &dyn Registry) -> State {
    let mut active = false;
    for s in item.settings {
        match reg.read(s.hive, s.path, s.name) {
            Err(_) => return State::Unknown,
            Ok(Some(RegValue::Dword(v))) if v == s.off_value => {}
            Ok(_) => active = true,
        }
    }
    if active {
        State::Active
    } else {
        State::Off
    }
}

fn applicable_of(item: &Item, env: Env) -> Applicable {
    // An unknown edition is given the benefit of the doubt.
    if env.edition != Edition::Unknown && env.edition < item.min_edition {
        Applicable::EditionNotSupported
    } else if item.needs_admin() && !env.elevated {
        Applicable::NeedsAdmin
    } else {
        Applicable::Yes
    }
}

/// Reads the real state of every item.
pub fn statuses(items: &[Item], reg: &dyn Registry, env: Env) -> Vec<Status> {
    items
        .iter()
        .map(|item| Status {
            id: item.id,
            state: state_of(item, reg),
            applicable: applicable_of(item, env),
        })
        .collect()
}

/// Counts for the ticked ids.
pub fn review(items: &[Item], statuses: &[Status], selected: &[&str]) -> Review {
    let mut r = Review::default();
    for item in items.iter().filter(|i| selected.contains(&i.id)) {
        let status = statuses.iter().find(|s| s.id == item.id);
        match status {
            Some(s)
                if s.state == State::Active && s.applicable != Applicable::EditionNotSupported =>
            {
                r.changes += 1;
                match item.risk {
                    Risk::Safe => r.safe += 1,
                    Risk::Careful => r.careful += 1,
                }
                if s.applicable == Applicable::NeedsAdmin {
                    r.needs_admin += 1;
                }
            }
            _ => r.skipped += 1,
        }
    }
    r
}

/// Puts one value back the way it was.
fn restore(op: &UndoOp, reg: &mut dyn Registry) -> Result<(), String> {
    match &op.previous {
        Some(v) => reg.write(op.hive, op.path, op.name, v),
        None => reg.delete(op.hive, op.path, op.name),
    }
}

/// Writes one item. On error, the item's already-written values are rolled back.
fn apply_item(item: &Item, reg: &mut dyn Registry, ops: &mut Vec<UndoOp>) -> Outcome {
    let mut written: Vec<UndoOp> = Vec::new();
    for s in item.settings {
        let previous = match reg.read(s.hive, s.path, s.name) {
            Ok(p) => p,
            Err(e) => return fail(e, written, reg),
        };
        let op = UndoOp {
            id: item.id,
            hive: s.hive,
            path: s.path,
            name: s.name,
            previous,
        };
        if let Err(e) = reg.write(s.hive, s.path, s.name, &RegValue::Dword(s.off_value)) {
            return fail(e, written, reg);
        }
        written.push(op);
    }

    let effective = item.settings.iter().all(|s| {
        matches!(
            reg.read(s.hive, s.path, s.name),
            Ok(Some(RegValue::Dword(v))) if v == s.off_value
        )
    });
    ops.extend(written);
    if effective {
        Outcome::Done
    } else {
        Outcome::NotEffective
    }
}

fn fail(error: String, written: Vec<UndoOp>, reg: &mut dyn Registry) -> Outcome {
    let mut message = error;
    for op in written.iter().rev() {
        if let Err(e) = restore(op, reg) {
            message.push_str(&format!(" (could not put {} back: {e})", op.name));
        }
    }
    Outcome::Failed(message)
}

/// Applies the ticked items (only those that are `Active` and applicable), recording the previous
/// values, and reads every value back to confirm it took effect.
pub fn apply(items: &[Item], selected: &[&str], reg: &mut dyn Registry, env: Env) -> Run {
    let mut run = Run::default();
    let current = statuses(items, &*reg, env);
    for item in items.iter().filter(|i| selected.contains(&i.id)) {
        let ready = current.iter().any(|s| {
            s.id == item.id && s.state == State::Active && s.applicable == Applicable::Yes
        });
        if !ready {
            continue;
        }
        let outcome = apply_item(item, reg, &mut run.undo);
        run.applied.push(Applied {
            id: item.id,
            outcome,
        });
    }
    run
}

/// Restores the recorded previous values (newest first). One result per undo op.
pub fn undo(run: &Run, reg: &mut dyn Registry) -> Vec<(&'static str, Result<(), String>)> {
    run.undo
        .iter()
        .rev()
        .map(|op| (op.id, restore(op, reg)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::debloat::backend::WinRegistry;
    use crate::tools::debloat::catalog;
    use crate::tools::debloat::model::{Group, RegSetting};
    use std::collections::HashMap;

    const P: &str = r"Software\Test";

    const fn user(name: &'static str) -> RegSetting {
        RegSetting {
            hive: Hive::CurrentUser,
            path: P,
            name,
            off_value: 0,
        }
    }

    fn item(
        id: &'static str,
        risk: Risk,
        min_edition: Edition,
        settings: &'static [RegSetting],
    ) -> Item {
        Item {
            id,
            group: Group::PrivacyAds,
            title: "t",
            what_changes: "w",
            could_break: "c",
            risk,
            min_edition,
            settings,
        }
    }

    const A: [RegSetting; 2] = [user("A1"), user("A2")];
    const B: [RegSetting; 1] = [user("B1")];
    const M: [RegSetting; 1] = [RegSetting {
        hive: Hive::LocalMachine,
        path: P,
        name: "M1",
        off_value: 1,
    }];

    fn test_items() -> Vec<Item> {
        vec![
            item("a", Risk::Safe, Edition::Home, &A),
            item("b", Risk::Safe, Edition::Home, &B),
            item("m", Risk::Careful, Edition::Pro, &M),
        ]
    }

    const PRO: Env = Env {
        edition: Edition::Pro,
        elevated: false,
    };
    const PRO_ADMIN: Env = Env {
        edition: Edition::Pro,
        elevated: true,
    };

    type Key = (u8, String, String);

    fn key(hive: Hive, path: &str, name: &str) -> Key {
        let h = if hive == Hive::CurrentUser { 0 } else { 1 };
        (h, path.to_string(), name.to_string())
    }

    #[derive(Default)]
    struct FakeRegistry {
        map: HashMap<Key, RegValue>,
        read_fails: Vec<String>,
        ignore_writes: bool,
        /// Fail the n-th write call (1-based).
        fail_write_at: Option<usize>,
        writes: usize,
    }

    impl FakeRegistry {
        fn set(&mut self, hive: Hive, name: &str, v: RegValue) {
            self.map.insert(key(hive, P, name), v);
        }
        fn get(&self, hive: Hive, name: &str) -> Option<RegValue> {
            self.map.get(&key(hive, P, name)).cloned()
        }
    }

    impl Registry for FakeRegistry {
        fn read(&self, hive: Hive, path: &str, name: &str) -> Result<Option<RegValue>, String> {
            if self.read_fails.iter().any(|n| n == name) {
                return Err("denied".to_string());
            }
            Ok(self.map.get(&key(hive, path, name)).cloned())
        }
        fn write(
            &mut self,
            hive: Hive,
            path: &str,
            name: &str,
            value: &RegValue,
        ) -> Result<(), String> {
            self.writes += 1;
            if self.fail_write_at == Some(self.writes) {
                return Err("write failed".to_string());
            }
            if !self.ignore_writes {
                self.map.insert(key(hive, path, name), value.clone());
            }
            Ok(())
        }
        fn delete(&mut self, hive: Hive, path: &str, name: &str) -> Result<(), String> {
            self.map.remove(&key(hive, path, name));
            Ok(())
        }
    }

    fn status_of(items: &[Item], reg: &FakeRegistry, env: Env, id: &str) -> Status {
        *statuses(items, reg, env)
            .iter()
            .find(|s| s.id == id)
            .unwrap()
    }

    #[test]
    fn missing_is_active() {
        let items = test_items();
        let reg = FakeRegistry::default();
        assert_eq!(status_of(&items, &reg, PRO, "a").state, State::Active);
    }

    #[test]
    fn all_off_is_off_and_partial_is_active() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "A1", RegValue::Dword(0));
        assert_eq!(status_of(&items, &reg, PRO, "a").state, State::Active);
        reg.set(Hive::CurrentUser, "A2", RegValue::Dword(0));
        assert_eq!(status_of(&items, &reg, PRO, "a").state, State::Off);
    }

    #[test]
    fn wrong_type_or_value_is_active() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "B1", RegValue::Sz("0".into()));
        assert_eq!(status_of(&items, &reg, PRO, "b").state, State::Active);
        reg.set(Hive::CurrentUser, "B1", RegValue::Dword(5));
        assert_eq!(status_of(&items, &reg, PRO, "b").state, State::Active);
    }

    #[test]
    fn read_error_is_unknown() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.read_fails.push("A2".into());
        assert_eq!(status_of(&items, &reg, PRO, "a").state, State::Unknown);
    }

    #[test]
    fn applicability() {
        let items = test_items();
        let reg = FakeRegistry::default();
        assert_eq!(
            status_of(&items, &reg, PRO, "a").applicable,
            Applicable::Yes
        );
        assert_eq!(
            status_of(&items, &reg, PRO, "m").applicable,
            Applicable::NeedsAdmin
        );
        assert_eq!(
            status_of(&items, &reg, PRO_ADMIN, "m").applicable,
            Applicable::Yes
        );
        let home = Env {
            edition: Edition::Home,
            elevated: true,
        };
        assert_eq!(
            status_of(&items, &reg, home, "m").applicable,
            Applicable::EditionNotSupported
        );
        let unknown = Env {
            edition: Edition::Unknown,
            elevated: true,
        };
        assert_eq!(
            status_of(&items, &reg, unknown, "m").applicable,
            Applicable::Yes
        );
    }

    #[test]
    fn review_counts() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "B1", RegValue::Dword(0)); // b already off
        let st = statuses(&items, &reg, PRO);
        let r = review(&items, &st, &["a", "b", "m", "nope"]);
        assert_eq!(
            r,
            Review {
                changes: 2,
                safe: 1,
                careful: 1,
                needs_admin: 1,
                skipped: 1,
            }
        );
        assert_eq!(review(&items, &st, &[]), Review::default());

        let home = Env {
            edition: Edition::Home,
            elevated: true,
        };
        let st = statuses(&items, &reg, home);
        let r = review(&items, &st, &["m"]);
        assert_eq!((r.changes, r.skipped), (0, 1));
    }

    #[test]
    fn apply_records_previous_and_flips_state() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "A1", RegValue::Dword(1));
        let run = apply(&items, &["a"], &mut reg, PRO);
        assert_eq!(
            run.applied,
            vec![Applied {
                id: "a",
                outcome: Outcome::Done
            }]
        );
        assert_eq!(run.undo.len(), 2);
        assert_eq!(run.undo[0].previous, Some(RegValue::Dword(1)));
        assert_eq!(run.undo[1].previous, None);
        assert_eq!(reg.get(Hive::CurrentUser, "A1"), Some(RegValue::Dword(0)));
        assert_eq!(reg.get(Hive::CurrentUser, "A2"), Some(RegValue::Dword(0)));
        assert_eq!(status_of(&items, &reg, PRO, "a").state, State::Off);
    }

    #[test]
    fn apply_ignores_unselected_off_and_inapplicable() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "B1", RegValue::Dword(0));
        let run = apply(&items, &["b", "m"], &mut reg, PRO);
        assert!(run.applied.is_empty());
        assert!(run.undo.is_empty());
        assert_eq!(reg.writes, 0);
        assert_eq!(reg.get(Hive::CurrentUser, "A1"), None);
    }

    #[test]
    fn apply_machine_item_when_elevated() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        let run = apply(&items, &["m"], &mut reg, PRO_ADMIN);
        assert_eq!(run.applied[0].outcome, Outcome::Done);
        assert_eq!(reg.get(Hive::LocalMachine, "M1"), Some(RegValue::Dword(1)));
    }

    #[test]
    fn apply_not_effective_when_writes_ignored() {
        let items = test_items();
        let mut reg = FakeRegistry {
            ignore_writes: true,
            ..Default::default()
        };
        let run = apply(&items, &["b"], &mut reg, PRO);
        assert_eq!(run.applied[0].outcome, Outcome::NotEffective);
        assert_eq!(run.undo.len(), 1);
    }

    #[test]
    fn apply_failure_rolls_back_item() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "A1", RegValue::Dword(1));
        reg.fail_write_at = Some(2);
        let run = apply(&items, &["a", "b"], &mut reg, PRO);
        assert!(matches!(run.applied[0].outcome, Outcome::Failed(_)));
        assert_eq!(run.applied[0].id, "a");
        // A1 is back to its old value, A2 was never written.
        assert_eq!(reg.get(Hive::CurrentUser, "A1"), Some(RegValue::Dword(1)));
        assert_eq!(reg.get(Hive::CurrentUser, "A2"), None);
        // The next item still ran and only its ops are recorded.
        assert_eq!(run.applied[1].outcome, Outcome::Done);
        assert!(run.undo.iter().all(|op| op.id == "b"));
    }

    #[test]
    fn undo_restores_exact_previous_and_deletes_new() {
        let items = test_items();
        let mut reg = FakeRegistry::default();
        reg.set(Hive::CurrentUser, "A1", RegValue::Dword(7));
        reg.set(Hive::CurrentUser, "B1", RegValue::Sz("keep".into()));
        let before = reg.map.clone();
        let run = apply(&items, &["a", "b"], &mut reg, PRO);
        assert_ne!(reg.map, before);
        let results = undo(&run, &mut reg);
        assert_eq!(results.len(), run.undo.len());
        assert!(results.iter().all(|(_, r)| r.is_ok()));
        // Newest first.
        assert_eq!(results[0].0, "b");
        assert_eq!(reg.map, before);
    }

    #[test]
    #[ignore = "reads the real registry"]
    fn smoke_real_registry_read_only() {
        let items = catalog::items();
        let st = statuses(
            items,
            &WinRegistry,
            crate::tools::debloat::backend::detect_env(),
        );
        assert_eq!(st.len(), items.len());
    }
}
