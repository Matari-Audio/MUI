"""Fetch the exact tested GPUI checkout without adding Zed to MUI's workspace."""
from pathlib import Path
import io,tarfile,urllib.request,subprocess,sys
REV='7960b2a7c9568e90fbe0727332149e5b2a5fd57a'
root=Path(__file__).resolve().parents[2]
target=root/'upstream'
if target.exists():
 marker=target/'MUI_SOURCE_REVISION'
 if not marker.exists() or marker.read_text().strip()!=REV:raise SystemExit('Existing upstream checkout is not marked with the pinned revision; move it aside first.')
 print('Existing pinned checkout retained:',REV)
else:
 with urllib.request.urlopen(f'https://codeload.github.com/zed-industries/zed/tar.gz/{REV}',timeout=120) as r:
  data=r.read()
 with tarfile.open(fileobj=io.BytesIO(data)) as t:t.extractall(root,filter='data')
 (root/f'zed-{REV}').rename(target)
 (target/'MUI_SOURCE_REVISION').write_text(REV+'\n')
script=Path(__file__).with_name('instrument_gpui.py')
source=target/'crates/gpui_wgpu/src/wgpu_renderer.rs'
if '// MUI LAB READBACK' not in source.read_text():subprocess.run([sys.executable,str(script)],check=True)
print('Prepared GPUI',REV)
