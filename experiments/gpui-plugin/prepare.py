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
        Ok(result?)
    }
}
'''
if addition not in p.read_text():
    p.write_text(p.read_text() + addition)
replace(base / 'gpui_linux.rs', 'pub use linux::current_platform;', 'pub use linux::current_platform;\n#[cfg(feature = "x11")]\npub use linux::EmbeddedX11;')
