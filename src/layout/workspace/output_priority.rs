//! Sway's workspace output priority list (`ws->output_priority`).
//!
//! A workspace remembers, highest first, the outputs it belongs on. Disabling an
//! output moves each workspace to the first enabled entry, and enabling one takes
//! back every workspace whose first enabled entry it is.

use super::*;

/// workspace_output_get_priority (sway/sway/tree/workspace.c:770-778): the
/// lower of the output's identifier and name positions.
fn index(priority: &[String], output: &Output) -> Option<usize> {
    let identifier = sway_output_identifier(output);
    let name = &output.user_data().get::<OutputName>().unwrap().connector;
    let by_id = priority.iter().position(|entry| *entry == identifier);
    let by_name = priority.iter().position(|entry| entry == name);
    match (by_id, by_name) {
        (Some(id), Some(name)) => Some(id.min(name)),
        (id, name) => id.or(name),
    }
}

/// workspace_output_add_priority (sway/sway/tree/workspace.c:799-806): append
/// the output's identifier at the lowest priority unless already listed.
pub(super) fn add(priority: &mut Vec<String>, output: &Output) {
    if index(priority, output).is_none() {
        priority.push(sway_output_identifier(output));
    }
}

impl<W: LayoutElement> Workspace<W> {
    pub(in crate::layout) fn add_output_priority(&mut self, output: &Output) {
        add(&mut self.output_priority, output);
    }

    /// Appends a raw entry, as sway does for the fallback output, whose
    /// identifier is the all-Unknown one (sway/sway/tree/output.c:240).
    pub(in crate::layout) fn add_output_priority_entry(&mut self, entry: String) {
        if !self.output_priority.contains(&entry) {
            self.output_priority.push(entry);
        }
    }

    /// workspace_output_raise_priority (sway/sway/tree/workspace.c:780-797):
    /// after `move workspace to output`, rank `new` just above `old`.
    pub(in crate::layout) fn raise_output_priority(&mut self, old: &Output, new: &Output) {
        let Some(old_index) = index(&self.output_priority, old) else {
            return;
        };
        match index(&self.output_priority, new) {
            None => self
                .output_priority
                .insert(old_index, sway_output_identifier(new)),
            Some(new_index) if new_index > old_index => {
                let entry = self.output_priority.remove(new_index);
                self.output_priority.insert(old_index, entry);
            }
            Some(_) => {}
        }
    }

    /// workspace_output_get_highest_available (sway/sway/tree/workspace.c:
    /// 808-819). `outputs` are the enabled outputs in sway's `root->outputs`
    /// order, which is the order they were enabled in.
    pub(in crate::layout) fn highest_available_output<'a>(
        &self,
        outputs: &'a [Output],
    ) -> Option<&'a Output> {
        self.output_priority.iter().find_map(|entry| {
            outputs
                .iter()
                .find(|output| sway_output_matches(output, entry))
        })
    }
}
