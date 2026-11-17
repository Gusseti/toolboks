//! The order of the add-ons in the sidebar, with the groups the user made. Pure data and rules:
//! the shell draws it and calls these to change it. The saved form lives in `Config`.
//!
//! The layout is a list of items: an add-on id, or a group with a name, a collapsed flag and its
//! add-on ids. Add-ons that are not installed or not enabled keep their slot but are never shown.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item {
    Addon(String),
    Group(Group),
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub name: String,
    pub collapsed: bool,
    pub ids: Vec<String>,
}

/// Where a dragged item lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drop {
    /// In front of this add-on, in whatever list it is in.
    Before(String),
    /// Behind this add-on, in whatever list it is in.
    After(String),
    /// In front of this group's header, at the top level.
    BeforeGroup(String),
    /// At the end of this group.
    IntoGroup(String),
    /// At the end of the top level.
    End,
}

/// A row of the sidebar as the user sees it: only enabled, installed add-ons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Addon(String),
    Group {
        name: String,
        collapsed: bool,
        /// The visible add-ons in the group.
        ids: Vec<String>,
    },
}

/// Adds the installed ids the layout does not know yet at the end of the top level, in the
/// order given, and drops a repeated id (the first one wins). Ids that are not installed stay.
pub fn normalize(layout: &mut Vec<Item>, installed: &[String]) {
    let mut seen = std::collections::HashSet::new();
    let mut changed = false;
    for item in layout.iter_mut() {
        match item {
            Item::Addon(id) => {
                if !seen.insert(id.clone()) {
                    changed = true;
                }
            }
            Item::Group(g) => {
                let before = g.ids.len();
                g.ids.retain(|id| seen.insert(id.clone()));
                changed |= g.ids.len() != before;
            }
        }
    }
    if changed {
        let mut seen = std::collections::HashSet::new();
        layout.retain_mut(|item| match item {
            Item::Addon(id) => seen.insert(id.clone()),
            Item::Group(g) => {
                g.ids.retain(|id| seen.insert(id.clone()));
                true
            }
        });
    }
    for id in installed {
        if !contains(layout, id) {
            layout.push(Item::Addon(id.clone()));
        }
    }
}

fn contains(layout: &[Item], id: &str) -> bool {
    layout.iter().any(|item| match item {
        Item::Addon(a) => a == id,
        Item::Group(g) => g.ids.iter().any(|a| a == id),
    })
}

/// The rows to draw. `visible` says whether an add-on is installed and enabled.
pub fn rows(layout: &[Item], visible: impl Fn(&str) -> bool) -> Vec<Row> {
    layout
        .iter()
        .filter_map(|item| match item {
            Item::Addon(id) => visible(id).then(|| Row::Addon(id.clone())),
            Item::Group(g) => Some(Row::Group {
                name: g.name.clone(),
                collapsed: g.collapsed,
                ids: g.ids.iter().filter(|id| visible(id)).cloned().collect(),
            }),
        })
        .collect()
}

/// The name of the group an add-on is in, if any.
pub fn group_of(layout: &[Item], id: &str) -> Option<String> {
    layout.iter().find_map(|item| match item {
        Item::Group(g) if g.ids.iter().any(|a| a == id) => Some(g.name.clone()),
        _ => None,
    })
}

pub fn group_names(layout: &[Item]) -> Vec<String> {
    layout
        .iter()
        .filter_map(|item| match item {
            Item::Group(g) => Some(g.name.clone()),
            _ => None,
        })
        .collect()
}

/// Takes the add-on out of wherever it is. Returns the top-level slot it left, if it was there.
fn detach(layout: &mut Vec<Item>, id: &str) -> Option<usize> {
    for item in layout.iter_mut() {
        if let Item::Group(g) = item {
            g.ids.retain(|a| a != id);
        }
    }
    let at = layout
        .iter()
        .position(|item| matches!(item, Item::Addon(a) if a == id))?;
    layout.remove(at);
    Some(at)
}

/// Moves an add-on. False when nothing changed.
pub fn move_addon(layout: &mut Vec<Item>, id: &str, to: &Drop) -> bool {
    if !contains(layout, id) {
        return false;
    }
    match to {
        Drop::Before(t) | Drop::After(t) if t == id => return false,
        Drop::Before(t) | Drop::After(t) if !contains(layout, t) => return false,
        Drop::BeforeGroup(g) | Drop::IntoGroup(g) if !group_names(layout).contains(g) => {
            return false
        }
        _ => {}
    }
    let before = layout.clone();
    detach(layout, id);
    let after = matches!(to, Drop::After(_));
    match to {
        Drop::Before(t) | Drop::After(t) => {
            let top = layout
                .iter()
                .position(|item| matches!(item, Item::Addon(a) if a == t));
            if let Some(i) = top {
                layout.insert(if after { i + 1 } else { i }, Item::Addon(id.to_owned()));
            } else {
                for item in layout.iter_mut() {
                    if let Item::Group(g) = item {
                        if let Some(i) = g.ids.iter().position(|a| a == t) {
                            g.ids.insert(if after { i + 1 } else { i }, id.to_owned());
                            break;
                        }
                    }
                }
            }
        }
        Drop::BeforeGroup(name) => {
            let i = layout
                .iter()
                .position(|item| matches!(item, Item::Group(g) if &g.name == name))
                .unwrap_or(layout.len());
            layout.insert(i, Item::Addon(id.to_owned()));
        }
        Drop::IntoGroup(name) => {
            for item in layout.iter_mut() {
                if let Item::Group(g) = item {
                    if &g.name == name {
                        g.ids.push(id.to_owned());
                    }
                }
            }
        }
        Drop::End => layout.push(Item::Addon(id.to_owned())),
    }
    *layout != before
}

/// Moves a whole group among the top-level items. Only top-level targets count; a target inside
/// another group means "in front of that group".
pub fn move_group(layout: &mut Vec<Item>, name: &str, to: &Drop) -> bool {
    let Some(from) = layout
        .iter()
        .position(|item| matches!(item, Item::Group(g) if g.name == name))
    else {
        return false;
    };
    let target = match to {
        Drop::End => None,
        Drop::BeforeGroup(t) | Drop::IntoGroup(t) => Some(
            layout
                .iter()
                .position(|item| matches!(item, Item::Group(g) if &g.name == t)),
        ),
        Drop::Before(t) | Drop::After(t) => Some(layout.iter().position(|item| match item {
            Item::Addon(a) => a == t,
            Item::Group(g) => g.ids.iter().any(|a| a == t),
        })),
    };
    let before = layout.clone();
    let item = layout.remove(from);
    let at = match target {
        None => layout.len(),
        Some(None) => {
            layout.insert(from, item);
            return false;
        }
        Some(Some(i)) => {
            // `i` was counted with the group still in place.
            let i = if i > from { i - 1 } else { i };
            if matches!(to, Drop::After(_)) {
                i + 1
            } else {
                i
            }
        }
    };
    layout.insert(at.min(layout.len()), item);
    *layout != before
}

/// Alt+Up / Alt+Down: swaps an add-on with the next visible one in its list. At the ends nothing
/// happens. `delta` is -1 or 1.
pub fn step_addon(
    layout: &mut [Item],
    id: &str,
    delta: i32,
    visible: impl Fn(&str) -> bool,
) -> bool {
    // The list the add-on is in: a group's ids, or the top level.
    for item in layout.iter_mut() {
        if let Item::Group(g) = item {
            if g.ids.iter().any(|a| a == id) {
                let slots: Vec<usize> = (0..g.ids.len()).filter(|&i| visible(&g.ids[i])).collect();
                return swap_in(&mut g.ids, &slots, |ids, i| ids[i] == id, delta);
            }
        }
    }
    let slots: Vec<usize> = layout
        .iter()
        .enumerate()
        .filter(|(_, item)| match item {
            Item::Addon(a) => visible(a),
            Item::Group(_) => true,
        })
        .map(|(i, _)| i)
        .collect();
    swap_in(
        layout,
        &slots,
        |items, i| matches!(&items[i], Item::Addon(a) if a == id),
        delta,
    )
}

/// Alt+Up / Alt+Down on a group header: swaps it with the next visible top-level item.
pub fn step_group(
    layout: &mut [Item],
    name: &str,
    delta: i32,
    visible: impl Fn(&str) -> bool,
) -> bool {
    let slots: Vec<usize> = layout
        .iter()
        .enumerate()
        .filter(|(_, item)| match item {
            Item::Addon(a) => visible(a),
            Item::Group(_) => true,
        })
        .map(|(i, _)| i)
        .collect();
    swap_in(
        layout,
        &slots,
        |items, i| matches!(&items[i], Item::Group(g) if g.name == name),
        delta,
    )
}

fn swap_in<T>(
    list: &mut [T],
    slots: &[usize],
    is_it: impl Fn(&[T], usize) -> bool,
    delta: i32,
) -> bool {
    let Some(pos) = slots.iter().position(|&i| is_it(list, i)) else {
        return false;
    };
    let next = pos as i32 + delta.signum();
    if next < 0 || next as usize >= slots.len() {
        return false;
    }
    list.swap(slots[pos], slots[next as usize]);
    true
}

fn check_name(layout: &[Item], name: &str, ignoring: Option<&str>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Type a name for the group.".into());
    }
    let taken = group_names(layout)
        .iter()
        .any(|g| g.eq_ignore_ascii_case(name) && Some(g.as_str()) != ignoring);
    if taken {
        return Err("A group with this name already exists.".into());
    }
    Ok(name.to_owned())
}

/// Makes a group at the end of the top level, with `id` in it when given.
pub fn new_group(layout: &mut Vec<Item>, name: &str, id: Option<&str>) -> Result<(), String> {
    let name = check_name(layout, name, None)?;
    let slot = id.and_then(|id| detach(layout, id));
    let group = Item::Group(Group {
        name,
        collapsed: false,
        ids: id.map(|id| vec![id.to_owned()]).unwrap_or_default(),
    });
    match slot {
        Some(i) => layout.insert(i, group),
        None => layout.push(group),
    }
    Ok(())
}

pub fn rename_group(layout: &mut [Item], old: &str, name: &str) -> Result<(), String> {
    let name = check_name(layout, name, Some(old))?;
    for item in layout.iter_mut() {
        if let Item::Group(g) = item {
            if g.name == old {
                g.name = name;
                break;
            }
        }
    }
    Ok(())
}

/// Removes the group; its add-ons take its place at the top level, in the same order.
pub fn delete_group(layout: &mut Vec<Item>, name: &str) -> bool {
    let Some(at) = layout
        .iter()
        .position(|item| matches!(item, Item::Group(g) if g.name == name))
    else {
        return false;
    };
    if let Item::Group(g) = layout.remove(at) {
        for (k, id) in g.ids.into_iter().enumerate() {
            layout.insert(at + k, Item::Addon(id));
        }
    }
    true
}

/// Takes an add-on out of its group, right behind the group's header.
pub fn remove_from_group(layout: &mut Vec<Item>, id: &str) -> bool {
    let Some(group) = group_of(layout, id) else {
        return false;
    };
    detach(layout, id);
    let at = layout
        .iter()
        .position(|item| matches!(item, Item::Group(g) if g.name == group))
        .map_or(layout.len(), |i| i + 1);
    layout.insert(at, Item::Addon(id.to_owned()));
    true
}

pub fn set_collapsed(layout: &mut [Item], name: &str, collapsed: bool) {
    for item in layout.iter_mut() {
        if let Item::Group(g) = item {
            if g.name == name {
                g.collapsed = collapsed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str) -> Item {
        Item::Addon(id.into())
    }
    fn g(name: &str, ids: &[&str]) -> Item {
        Item::Group(Group {
            name: name.into(),
            collapsed: false,
            ids: ids.iter().map(|s| (*s).into()).collect(),
        })
    }
    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).into()).collect()
    }
    fn all(_: &str) -> bool {
        true
    }

    #[test]
    fn new_ids_go_last_and_unknown_ids_are_kept() {
        let mut l = vec![a("c"), g("G", &["x", "a"]), a("gone")];
        normalize(&mut l, &ids(&["a", "b", "c", "x"]));
        assert_eq!(l, vec![a("c"), g("G", &["x", "a"]), a("gone"), a("b")]);
        // Not shown: "gone" is not installed.
        let shown = rows(&l, |id| id != "gone");
        assert_eq!(
            shown,
            vec![
                Row::Addon("c".into()),
                Row::Group {
                    name: "G".into(),
                    collapsed: false,
                    ids: ids(&["x", "a"])
                },
                Row::Addon("b".into()),
            ]
        );
    }

    #[test]
    fn empty_layout_follows_the_installed_order_and_repeats_are_dropped() {
        let mut l = vec![];
        normalize(&mut l, &ids(&["a", "b"]));
        assert_eq!(l, vec![a("a"), a("b")]);
        let mut l = vec![a("a"), g("G", &["a", "b"]), a("b")];
        normalize(&mut l, &ids(&["a", "b"]));
        assert_eq!(l, vec![a("a"), g("G", &["b"])]);
    }

    #[test]
    fn disabled_add_ons_keep_their_slot() {
        let mut l = vec![a("a"), a("b"), a("c")];
        let off = |id: &str| id != "b";
        assert_eq!(rows(&l, off).len(), 2);
        assert!(move_addon(&mut l, "c", &Drop::Before("a".into())));
        assert_eq!(l, vec![a("c"), a("a"), a("b")]);
        // Stepping skips the hidden one but leaves it in place.
        let mut l = vec![a("a"), a("b"), a("c")];
        assert!(step_addon(&mut l, "a", 1, off));
        assert_eq!(l, vec![a("c"), a("b"), a("a")]);
    }

    #[test]
    fn moves_at_the_ends_do_nothing() {
        let mut l = vec![a("a"), a("b"), a("c")];
        assert!(!step_addon(&mut l, "a", -1, all));
        assert!(!step_addon(&mut l, "c", 1, all));
        assert_eq!(l, vec![a("a"), a("b"), a("c")]);
        assert!(step_addon(&mut l, "b", -1, all));
        assert_eq!(l, vec![a("b"), a("a"), a("c")]);
        assert!(step_addon(&mut l, "b", 1, all));
        assert_eq!(l, vec![a("a"), a("b"), a("c")]);
        let mut l = vec![g("G", &["x"]), a("a")];
        assert!(!step_group(&mut l, "G", -1, all));
        assert!(step_group(&mut l, "G", 1, all));
        assert_eq!(l, vec![a("a"), g("G", &["x"])]);
        assert!(!step_group(&mut l, "G", 1, all));
        let mut l = vec![g("G", &["x", "y"])];
        assert!(!step_addon(&mut l, "x", -1, all));
        assert!(!step_addon(&mut l, "y", 1, all));
        assert!(step_addon(&mut l, "y", -1, all));
        assert_eq!(l, vec![g("G", &["y", "x"])]);
    }

    #[test]
    fn drag_into_a_group_between_rows_and_out_again() {
        let mut l = vec![a("a"), g("G", &["x", "y"]), a("b")];
        assert!(move_addon(&mut l, "a", &Drop::IntoGroup("G".into())));
        assert_eq!(l, vec![g("G", &["x", "y", "a"]), a("b")]);
        assert!(move_addon(&mut l, "b", &Drop::Before("y".into())));
        assert_eq!(l, vec![g("G", &["x", "b", "y", "a"])]);
        assert!(move_addon(&mut l, "x", &Drop::End));
        assert_eq!(l, vec![g("G", &["b", "y", "a"]), a("x")]);
        assert!(move_addon(&mut l, "a", &Drop::BeforeGroup("G".into())));
        assert_eq!(l, vec![a("a"), g("G", &["b", "y"]), a("x")]);
        assert!(move_addon(&mut l, "a", &Drop::After("x".into())));
        assert_eq!(l, vec![g("G", &["b", "y"]), a("x"), a("a")]);
        // Dropping on itself or on something unknown changes nothing.
        assert!(!move_addon(&mut l, "a", &Drop::Before("a".into())));
        assert!(!move_addon(&mut l, "a", &Drop::IntoGroup("nope".into())));
        assert!(!move_addon(&mut l, "ghost", &Drop::End));
        assert!(!move_addon(&mut l, "a", &Drop::End));
    }

    #[test]
    fn groups_move_among_top_level_items() {
        let mut l = vec![a("a"), g("G", &["x"]), g("H", &["y"]), a("b")];
        assert!(move_group(&mut l, "H", &Drop::BeforeGroup("G".into())));
        assert_eq!(l, vec![a("a"), g("H", &["y"]), g("G", &["x"]), a("b")]);
        assert!(move_group(&mut l, "H", &Drop::End));
        assert_eq!(l, vec![a("a"), g("G", &["x"]), a("b"), g("H", &["y"])]);
        assert!(move_group(&mut l, "H", &Drop::Before("a".into())));
        assert_eq!(l, vec![g("H", &["y"]), a("a"), g("G", &["x"]), a("b")]);
        // Behind an add-on in another group means behind that group.
        assert!(move_group(&mut l, "H", &Drop::After("x".into())));
        assert_eq!(l, vec![a("a"), g("G", &["x"]), g("H", &["y"]), a("b")]);
        assert!(!move_group(&mut l, "H", &Drop::BeforeGroup("H".into())));
        assert!(!move_group(&mut l, "H", &Drop::Before("nope".into())));
        assert_eq!(l, vec![a("a"), g("G", &["x"]), g("H", &["y"]), a("b")]);
    }

    #[test]
    fn new_rename_delete_and_remove() {
        let mut l = vec![a("a"), a("b"), a("c")];
        assert!(new_group(&mut l, "  ", Some("b")).is_err());
        new_group(&mut l, " Tools ", Some("b")).unwrap();
        assert_eq!(l, vec![a("a"), g("Tools", &["b"]), a("c")]);
        assert!(new_group(&mut l, "tools", None).is_err());
        assert_eq!(group_of(&l, "b").as_deref(), Some("Tools"));
        new_group(&mut l, "Empty", None).unwrap();
        assert_eq!(l.last(), Some(&g("Empty", &[])));
        assert!(rename_group(&mut l, "Tools", "empty").is_err());
        rename_group(&mut l, "Tools", "TOOLS").unwrap();
        // Renaming to the same name with other case is allowed for the group itself.
        assert_eq!(group_names(&l), ["TOOLS", "Empty"].map(String::from));
        assert!(move_addon(&mut l, "c", &Drop::IntoGroup("TOOLS".into())));
        assert!(remove_from_group(&mut l, "b"));
        assert_eq!(l, vec![a("a"), g("TOOLS", &["c"]), a("b"), g("Empty", &[])]);
        assert!(!remove_from_group(&mut l, "b"));
        assert!(move_addon(&mut l, "b", &Drop::IntoGroup("TOOLS".into())));
        assert!(delete_group(&mut l, "TOOLS"));
        assert_eq!(l, vec![a("a"), a("c"), a("b"), g("Empty", &[])]);
        assert!(!delete_group(&mut l, "TOOLS"));
    }

    #[test]
    fn collapse_is_saved_and_old_configs_load() {
        let mut l = vec![g("G", &["x"]), a("a")];
        set_collapsed(&mut l, "G", true);
        let text = serde_json::to_string(&l).unwrap();
        let back: Vec<Item> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, l);
        assert!(matches!(&back[0], Item::Group(g) if g.collapsed));
        let plain: Vec<Item> = serde_json::from_str(r#"["a",{"name":"G","ids":["x"]}]"#).unwrap();
        assert_eq!(plain, vec![a("a"), g("G", &["x"])]);
    }
}
