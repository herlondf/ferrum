//! What some shortcuts open: New tag (Ctrl+Shift+G) and Delete on the
//! sidebar's selected ref.

use ferrum_core::BranchKind;

use crate::command::Op;
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::focus::Pane;

/// New tag at the commit selected in the history, else at HEAD.
pub fn new_tag(tab: &RepoTab) -> Dialog {
    let selected = tab
        .history
        .selected
        .filter(|_| tab.view == View::History)
        .and_then(|id| tab.index.get(&id).map(|&i| &tab.commits[i]));
    match selected {
        Some(c) => Dialog::create_tag(&c.id.to_string(), &c.short_id()),
        None => {
            let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
            Dialog::create_tag("HEAD", &head)
        }
    }
}

/// The confirmation to delete the branch, tag or stash under the sidebar
/// cursor (or the selected branches), when the sidebar has the keyboard.
pub fn delete_ref(tab: &RepoTab) -> Option<Dialog> {
    if tab.pane.pane != Pane::Sidebar || tab.busy() {
        return None;
    }
    let st = &tab.sidebar;
    let key = st.cursor.as_deref()?;
    if let Some(name) = key.strip_prefix("ref:refs/heads/") {
        let mut names: Vec<String> = if st.selection.contains(name) {
            st.selection.iter().cloned().collect()
        } else {
            vec![name.to_owned()]
        };
        names.retain(|n| tab.head.as_ref() != Some(n));
        return (!names.is_empty()).then(|| crate::ui::ref_dialogs::delete_branches(tab, &names));
    }
    if let Some(name) = key.strip_prefix("ref:refs/tags/") {
        return Some(Dialog::DeleteTag {
            name: name.to_owned(),
            remote: None,
        });
    }
    if let Some(full) = key.strip_prefix("ref:refs/remotes/") {
        let b = tab
            .branches
            .iter()
            .find(|b| b.kind == BranchKind::Remote && b.name == full)?;
        let (remote, name) = b.split_remote()?;
        #[rustfmt::skip]
        let text = t!("Delete the branch {} from {}? Other people using it will lose it.", name, remote);
        return Some(Dialog::confirm(
            &t!("Delete remote branch"),
            &text,
            Op::DeleteRemoteRef {
                remote: remote.to_owned(),
                name: name.to_owned(),
            },
            true,
        ));
    }
    if let Some(id) = key.strip_prefix("stash:") {
        let s = tab.stashes.iter().find(|s| s.id.to_string() == id)?;
        let (r, m) = (s.refname(), &s.message);
        let text = t!("Delete {} ({})? This cannot be undone.", r, m);
        return Some(Dialog::confirm(
            &t!("Drop stash"),
            &text,
            Op::StashDrop(s.clone()),
            true,
        ));
    }
    None
}
