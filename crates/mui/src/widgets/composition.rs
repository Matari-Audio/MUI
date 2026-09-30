//! Small semantic compositions, built from the same intrinsic rows and columns.
use std::sync::Arc;

use mui_scene::prelude::*;

use super::{Control, Response, card, toggle};
use crate::Ui;

/// A titled intrinsic group. Its id owns its semantic children; its title is
/// its accessible name. No fixed width, fill-parent rule, or new layout solver.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let panel = group("actions", "Project", [button(&mut ui, "save", "Save")]);
/// let scene = resolve(&SceneSpec::new(panel)).unwrap();
/// assert_eq!(scene.surface("save").unwrap().parent.as_deref(), Some("actions"));
/// ```
pub fn group(
    id: impl Into<Id>,
    label: impl Into<Arc<str>>,
    children: impl IntoIterator<Item = impl IntoEl>,
) -> El {
    let label = label.into();
    col([
        title(label.clone()),
        col(children.into_iter().map(IntoEl::into_el)).gap(S),
    ])
    .gap(M)
    .pad(M)
    .preset(card())
    .a11y(A11y::Group)
    .named(label)
    .id(id)
}

/// Label, optional help, and an input. The control id is supplied once; the
/// surrounding semantic group gets its own derived id. Finish with `toggle`
/// or `control`; the response keeps the original `changed` result.
pub struct Setting {
    id: Id,
    label: Arc<str>,
    description: Option<Arc<str>>,
}

/// Begin a labelled setting. Help text is both visible and attached to the
/// input's accessible description, without becoming part of its name.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut enabled = true;
/// let row = setting("sync", "Sync")
///     .description("Follow the host tempo")
///     .toggle(&mut ui, &mut enabled);
/// assert!(!row.changed);
/// ```
pub fn setting(id: impl Into<Id>, label: impl Into<Arc<str>>) -> Setting {
    Setting {
        id: id.into(),
        label: label.into(),
        description: None,
    }
}

impl Setting {
    pub fn description(mut self, text: impl Into<Arc<str>>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// A switch with a visible label and accessible help.
    pub fn toggle(self, ui: &mut Ui, value: &mut bool) -> Response<bool, El> {
        self.control(ui, |ui, id, label| toggle(ui, id, label, value))
    }

    /// Compose any built-in control. The callback receives the setting's
    /// stable id and label, so neither needs repeating at the call site.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut gain = 0.5;
    /// let row = setting("gain", "Gain").control(&mut ui, |ui, id, label| {
    ///     knob(ui, id, label, &mut gain, 0.0..=1.0).size(S)
    /// });
    /// ```
    pub fn control<C>(
        self,
        ui: &mut Ui,
        make: impl FnOnce(&mut Ui, Id, &str) -> Response<C, Control>,
    ) -> Response<C, El> {
        let group_id = self.id.field("setting");
        let Response { mut el, changed } = make(ui, self.id, &self.label);
        el = el.hide_matching_adjustment_label(&self.label);
        let mut labels = vec![body(self.label.clone())];
        if let Some(description) = self.description {
            el = el.described(description.clone());
            labels.push(caption(description).fill(Role::Dim));
        }
        Response {
            changed,
            el: row([col(labels).gap(Xs), spacer(), el.el()])
                .gap(M)
                .align(Align::Center)
                .a11y(A11y::Group)
                .named(self.label)
                .id(group_id),
        }
    }
}
