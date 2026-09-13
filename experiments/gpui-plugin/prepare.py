"""Add a host-driven X11 pump to the pinned experimental GPUI checkout."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent
subprocess.run([sys.executable, str(root.parent / 'render-lab/tools/prepare.py')], check=True)
base = root.parent / 'upstream/crates/gpui_linux/src'

def replace(path, old, new):
    text = path.read_text()
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'Pinned source mismatch: {path}')
    path.write_text(text.replace(old, new))

replace(base / 'linux/platform.rs', 'pub(crate) inner: P,', 'pub(crate) inner: P,\n    pub(crate) embedded: bool,')
replace(base / 'linux/platform.rs', '        LinuxClient::run(&self.inner);', '        if self.embedded { return; }\n        LinuxClient::run(&self.inner);')
replace(base / 'linux/x11/client.rs', 'pub(crate) struct WindowRef {\n    window:', 'pub(crate) struct WindowRef {\n    pub(crate) window:')
p = base / 'linux.rs'
s = p.read_text()
if 'embedded: false' not in s:
    assert s.count('inner: ') == 4
    p.write_text(s.replace('inner: ', 'embedded: false,\n            inner: '))
addition = '''
/// Experimental X11 runtime driven by the editor's UI thread.
#[cfg(feature = "x11")]
pub struct EmbeddedX11(X11Client);

#[cfg(feature = "x11")]
impl EmbeddedX11 {
    pub fn new() -> anyhow::Result<Self> { Ok(Self(X11Client::new()?)) }
    pub fn platform(&self) -> Rc<dyn gpui::Platform> {
        Rc::new(LinuxPlatform { inner: self.0.clone(), embedded: true })
    }
    pub fn pump(&self) -> anyhow::Result<()> {
        let mut event_loop = self.0.0.borrow_mut().event_loop.take()
            .ok_or_else(|| anyhow::anyhow!("Reentrant GPUI event pump"))?;
        let result = event_loop.dispatch(Some(std::time::Duration::ZERO), &mut self.0.clone());
        self.0.0.borrow_mut().event_loop = Some(event_loop);
        result?;
        // Child windows cannot rely on the compositor's top-level frame wakeups.
        let windows: Vec<_> = self.0.0.borrow().windows.values().map(|w| w.window.clone()).collect();
        for window in windows { window.refresh(gpui::RequestFrameOptions::default()); }
        Ok(())
    }
}
'''
current = p.read_text()
marker = '\n/// Experimental X11 runtime driven by the editor'
assert current.count(marker) <= 1
updated = current.split(marker)[0] + addition
if current != updated:
    p.write_text(updated)
replace(base / 'gpui_linux.rs', 'pub use linux::current_platform;', 'pub use linux::current_platform;\n#[cfg(feature = "x11")]\npub use linux::EmbeddedX11;')

# Reuse the pinned GPUI text-input example, excluding its standalone application.
source = (root.parent / 'upstream/crates/gpui/examples/input.rs').read_text()
source = source[source.index('use std::ops::Range;'):source.index('struct InputExample {')]
source = source.replace('use gpui_platform::application;\n', '')
source = source.replace('.key_context("TextInput")', '.id("preset-name").key_context("TextInput").tab_index(1).role(gpui::Role::TextInput).aria_label("Preset name")')
source = source.replace('struct TextInput {', 'pub struct TextInput {', 1)
# The standalone example's reset button is not part of the reusable field.
reset_start = source.index('    fn reset(&mut self) {')
reset_end = source.index('\n    }', reset_start) + len('\n    }')
source = source[:reset_start] + source[reset_end:]
source += """
impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { focus_handle: cx.focus_handle().tab_index(1).tab_stop(true), content: "".into(),
            placeholder: "Preset name…".into(), selected_range: 0..0,
            selection_reversed: false, marked_range: None, last_layout: None,
            last_bounds: None, is_selecting: false }
    }
    pub fn value(&self) -> &str { &self.content }
}
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
    ]);
}
"""
(root.parent / 'upstream/mui_text_input.rs').write_text('#![allow(unused_imports)]\n' + source)
