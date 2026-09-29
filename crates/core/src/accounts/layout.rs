//! The layout: one leader, the accounts that auto-join its server in order,
//! and named groups. Kept on the accounts -- `leader`, `follow` (place in the
//! join order), `group` (a group id) -- so a rename carries them along; the
//! groups themselves are groups.json.

use chrono::{DateTime, Utc};

use super::{Account, AccountError, AccountStore, Group};
use crate::types::{PlaceId, UserId};

impl AccountStore {
    pub fn leader(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.leader)
    }

    /// The auto-join order. The leader never follows itself.
    pub fn followers(&self) -> Vec<&Account> {
        let mut f: Vec<&Account> =
            self.accounts.iter().filter(|a| !a.leader && a.follow.is_some_and(|n| n > 0)).collect();
        f.sort_by_key(|a| a.follow);
        f
    }

    fn follower_ids(&self) -> Vec<UserId> {
        self.followers().iter().map(|a| a.user_id).collect()
    }

    /// `id` leads, and is selected; it leaves the auto-join list. Its group
    /// stays on it, so it goes back there when another account leads.
    pub fn make_leader(&mut self, id: UserId) {
        for a in &mut self.accounts {
            a.leader = a.user_id == id;
            if a.leader {
                a.selected = true;
                a.follow = None;
            }
        }
        let order = self.follower_ids();
        self.renumber(&order);
    }

    /// Put `id` at the end of the auto-join order, or take it out.
    pub fn set_follow(&mut self, id: UserId, on: bool) {
        let mut order: Vec<UserId> = self.follower_ids().into_iter().filter(|f| *f != id).collect();
        if on && self.get(id).is_some_and(|a| !a.leader) {
            order.push(id);
        }
        self.renumber(&order);
    }

    /// Move a follower up (negative) or down the join order.
    pub fn move_follower(&mut self, id: UserId, delta: isize) {
        let mut order = self.follower_ids();
        let Some(i) = order.iter().position(|f| *f == id) else { return };
        match i.checked_add_signed(delta) {
            Some(j) if j < order.len() => order.swap(i, j),
            _ => return,
        }
        self.renumber(&order);
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// The account's group, if it names one that exists.
    pub fn group_of(&self, a: &Account) -> Option<&str> {
        let id = a.group.as_deref()?;
        self.groups.iter().find(|g| g.id == id).map(|g| g.id.as_str())
    }

    /// Everyone but the leader, as drawn: group by group, then the
    /// ungrouped, each in list order.
    pub fn visual_order(&self) -> Vec<&Account> {
        let rest: Vec<&Account> = self.accounts.iter().filter(|a| !a.leader).collect();
        let sections = self.groups.iter().map(|g| Some(g.id.as_str())).chain([None]);
        sections
            .flat_map(|section| rest.iter().copied().filter(move |a| self.group_of(a) == section))
            .collect()
    }

    /// The selected accounts: the leader first, then in drawn order.
    pub fn selected(&self) -> Vec<&Account> {
        self.leader().into_iter().chain(self.visual_order()).filter(|a| a.selected).collect()
    }

    /// Dragging `id` onto the row of `onto`: it takes that row's group and its
    /// place in the drawn order -- after it when dragged down, before when up.
    /// The list is rewritten in drawn order, so that is what it keeps. The
    /// leader is not dragged anywhere.
    pub fn drop_on(&mut self, id: UserId, onto: UserId) {
        let (Some(moved), Some(target)) = (self.get(id), self.get(onto)) else { return };
        if id == onto || moved.leader {
            return;
        }
        let group = target.group.clone();
        let mut order: Vec<UserId> = self.visual_order().iter().map(|a| a.user_id).collect();
        let (Some(from), Some(to)) =
            (order.iter().position(|a| *a == id), order.iter().position(|a| *a == onto))
        else {
            return;
        };
        order.remove(from);
        order.insert(to, id);
        let mut accounts = std::mem::take(&mut self.accounts);
        accounts.sort_by_key(|a| (!a.leader, order.iter().position(|o| *o == a.user_id)));
        self.accounts = accounts;
        if let Ok(a) = self.get_mut(id) {
            a.group = group;
        }
    }

    /// Move `id` into a group (or out of all), at the group's end.
    pub fn set_group(&mut self, id: UserId, group: Option<&str>) -> Result<(), AccountError> {
        let at = self
            .accounts
            .iter()
            .position(|a| a.user_id == id)
            .ok_or(AccountError::NoSuchAccount)?;
        if self.accounts[at].leader {
            return Err(AccountError::LeaderInGroup);
        }
        let mut a = self.accounts.remove(at);
        a.group = group.map(str::to_owned);
        self.accounts.push(a);
        Ok(())
    }

    /// A new, open, empty group. Returns its id.
    pub fn add_group(&mut self, now: DateTime<Utc>) -> String {
        let base = format!("g{}", now.timestamp_millis());
        let mut id = base.clone();
        let mut n = 2;
        while self.groups.iter().any(|g| g.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.groups.push(Group {
            id: id.clone(),
            name: "New group".into(),
            place_id: None,
            game: None,
            open: true,
            extra: Default::default(),
        });
        id
    }

    /// Delete a group; its accounts become ungrouped.
    pub fn delete_group(&mut self, id: &str) -> Option<Group> {
        let at = self.groups.iter().position(|g| g.id == id)?;
        for a in self.accounts.iter_mut().filter(|a| a.group.as_deref() == Some(id)) {
            a.group = None;
        }
        Some(self.groups.remove(at))
    }

    pub fn rename_group(&mut self, id: &str, name: &str) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            name.clone_into(&mut g.name);
        }
    }

    pub fn set_group_open(&mut self, id: &str, open: bool) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            g.open = open;
        }
    }

    /// The game a group launches into, or none.
    pub fn set_group_game(&mut self, id: &str, game: Option<(PlaceId, String)>) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            (g.place_id, g.game) = game.map_or((None, None), |(p, n)| (Some(p), Some(n)));
        }
    }

    /// The layout before groups: the first selected account led and the
    /// rest selected followed it. Nothing changes if anyone leads or follows.
    pub(super) fn migrate_layout(&mut self) {
        if self.accounts.iter().any(|a| a.leader || a.follow.is_some()) {
            return;
        }
        let chosen: Vec<UserId> =
            self.accounts.iter().filter(|a| a.selected).map(|a| a.user_id).collect();
        if let Some((first, rest)) = chosen.split_first() {
            self.make_leader(*first);
            self.renumber(rest);
        }
    }

    fn renumber(&mut self, order: &[UserId]) {
        for a in &mut self.accounts {
            a.follow = order.iter().position(|id| *id == a.user_id).map(|i| i as u32 + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::store;
    use super::*;

    /// alt1..alt4 with ids 1..4; alt3 unselected, as in the Python checks.
    fn four() -> AccountStore {
        store(&[("alt1", true), ("alt2", true), ("alt3", false), ("alt4", true)], &[])
    }

    fn names(accts: Vec<&Account>) -> Vec<&str> {
        accts.into_iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn the_old_layout_carries_over() {
        let mut s = four();
        s.migrate_layout();
        assert_eq!(s.leader().map(|a| a.name.as_str()), Some("alt1"));
        assert_eq!(names(s.followers()), ["alt2", "alt4"]);
        s.migrate_layout();
        assert_eq!(names(s.followers()), ["alt2", "alt4"], "it only happens once");
    }

    #[test]
    fn a_new_leader_is_selected_and_the_only_one() {
        let mut s = four();
        s.migrate_layout();
        s.make_leader(UserId(3));
        assert_eq!(s.leader().map(|a| a.user_id), Some(UserId(3)));
        assert!(s.get(UserId(3)).unwrap().selected);
        assert_eq!(s.accounts().iter().filter(|a| a.leader).count(), 1);
        assert_eq!(names(s.followers()), ["alt2", "alt4"]);
    }

    #[test]
    fn a_follower_made_leader_stops_following_and_the_rest_renumber() {
        let mut s = four();
        s.migrate_layout();
        s.make_leader(UserId(2));
        let order: Vec<(&str, Option<u32>)> =
            s.followers().iter().map(|a| (a.name.as_str(), a.follow)).collect();
        assert_eq!(order, [("alt4", Some(1))]);
    }

    #[test]
    fn the_leader_cannot_follow_and_others_join_at_the_end() {
        let mut s = four();
        s.migrate_layout();
        s.make_leader(UserId(2));
        s.set_follow(UserId(1), true);
        s.set_follow(UserId(2), true);
        assert_eq!(names(s.followers()), ["alt4", "alt1"]);
        s.move_follower(UserId(1), -1);
        assert_eq!(names(s.followers()), ["alt1", "alt4"]);
        s.move_follower(UserId(1), -1);
        assert_eq!(names(s.followers()), ["alt1", "alt4"], "the first cannot move further up");
        s.set_follow(UserId(1), false);
        let order: Vec<(&str, Option<u32>)> =
            s.followers().iter().map(|a| (a.name.as_str(), a.follow)).collect();
        assert_eq!(order, [("alt4", Some(1))]);
    }

    fn grouped() -> AccountStore {
        let mut s = store(
            &[("alt1", true), ("alt2", true), ("alt3", false), ("alt4", true)],
            &[("g1", "Farm"), ("g2", "Event")],
        );
        s.accounts[1].group = Some("g2".into());
        s.accounts[2].group = Some("g1".into());
        s.accounts[3].group = Some("gone".into());
        s.make_leader(UserId(1));
        s
    }

    #[test]
    fn drawn_order_is_group_by_group_then_the_ungrouped() {
        assert_eq!(names(grouped().visual_order()), ["alt3", "alt2", "alt4"]);
    }

    #[test]
    fn a_dropped_row_takes_the_group_and_place_of_the_row_it_lands_on() {
        let mut s = grouped();
        s.drop_on(UserId(4), UserId(3));
        assert_eq!(names(s.visual_order()), ["alt4", "alt3", "alt2"], "dragged up: before it");
        assert_eq!(s.get(UserId(4)).unwrap().group.as_deref(), Some("g1"));
        s.drop_on(UserId(4), UserId(2));
        assert_eq!(names(s.visual_order()), ["alt3", "alt2", "alt4"], "dragged down: after it");
    }

    #[test]
    fn the_leader_is_not_dragged_anywhere() {
        let mut s = grouped();
        let before = s.accounts().to_vec();
        s.drop_on(UserId(1), UserId(2));
        assert_eq!(s.accounts(), before.as_slice());
    }

    #[test]
    fn selected_is_the_leader_then_the_drawn_order() {
        let s = grouped();
        assert_eq!(names(s.selected()), ["alt1", "alt2", "alt4"]);
    }

    #[test]
    fn the_leader_cannot_join_a_group() {
        let mut s = grouped();
        assert_eq!(s.set_group(UserId(1), Some("g1")), Err(AccountError::LeaderInGroup));
        s.set_group(UserId(2), Some("g1")).unwrap();
        assert_eq!(names(s.visual_order()), ["alt3", "alt2", "alt4"], "at the group's end");
    }

    #[test]
    fn groups_are_added_and_deleted_without_losing_accounts() {
        let mut s = grouped();
        let now = chrono::Utc::now();
        let a = s.add_group(now);
        let b = s.add_group(now);
        assert_ne!(a, b, "two groups added at once still get their own ids");
        assert!(s.groups().iter().any(|g| g.id == a && g.open && g.name == "New group"));
        s.delete_group("g1");
        assert_eq!(s.get(UserId(3)).unwrap().group, None);
        assert_eq!(names(s.visual_order()), ["alt2", "alt3", "alt4"]);
    }
}
