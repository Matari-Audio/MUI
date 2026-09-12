"""Package the two Rust-rendered states into a self-contained comparison."""
from pathlib import Path
import xml.etree.ElementTree as ET
root = Path(__file__).resolve().parent.parent
normal = (root / 'docs/items.svg').read_text()
hover = (root / 'docs/items-hover.svg').read_text()
html = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>MUI hover comparison</title>
<style>body{margin:0;background:#eee;color:#181818;font:16px system-ui}main{max-width:1000px;margin:auto;padding:24px}h1{font-size:24px}p{line-height:1.5;max-width:780px}.stage{position:relative;max-width:860px}.stage svg{display:block;width:100%;height:auto;pointer-events:none}.hover{position:absolute;inset:0;opacity:0}.stage.active .hover{opacity:1}.stage.active .normal{opacity:0}label{display:block;margin:18px 0;padding:12px;background:white;width:fit-content;border-radius:8px}input{width:20px;height:20px;vertical-align:middle}summary{cursor:pointer}code{background:#fff;padding:2px 6px}</style>
<main><h1>The same shape, two fill colors</h1><p>Move over <strong>Filter</strong> in either theme, or use the checkbox on touch screens. The tab and panel are already merged in both states. Hover changes their shared fill; it does not create the shape. The text stays readable and usually keeps the same color.</p>
<label><input id="pin" type="checkbox"> Show hovered state</label>
<div class="stage" id="stage"><div class="normal">NORMAL</div><div class="hover">HOVER</div></div>
<p id="status" role="status">Normal</p><p>These are the exact normal and hover SVGs exported by the Rust example. This page switches between them; it does not run the Rust engine in the browser.</p>
<details><summary>What changes?</summary><p>Light mode darkens the fill. Dark mode brightens it. Content position and shape geometry are unchanged. The decorative bridge is outside the hover hit area. Hover states need readable text, but WCAG does not require a 3:1 difference between old and new fills.</p></details></main>
<script>const stage=document.querySelector('#stage'),pin=document.querySelector('#pin'),status=document.querySelector('#status');let over=false;function update(){const active=pin.checked||over;stage.classList.toggle('active',active);status.textContent=active?'Hovered — the shared fill changed':'Normal';}pin.addEventListener('change',update);stage.addEventListener('pointermove',e=>{over=[...stage.querySelectorAll('.normal [data-hover-target]')].some(el=>{const r=el.getBoundingClientRect();return e.clientX>=r.left&&e.clientX<r.right&&e.clientY>=r.top&&e.clientY<r.bottom;});update();});stage.addEventListener('pointerleave',()=>{over=false;update();});</script></html>'''.replace('NORMAL', normal).replace('HOVER', hover)
(root / 'docs/hover-demo.html').write_text(html)
ET.register_namespace('', 'http://www.w3.org/2000/svg')
svg = ET.Element('{http://www.w3.org/2000/svg}svg', width='860', height='720', viewBox='0 0 860 720')
for y, source in [(0,normal),(360,hover)]:
    group = ET.SubElement(svg,'{http://www.w3.org/2000/svg}g',transform=f'translate(0,{y})')
    for child in ET.fromstring(source): group.append(child)
(root / 'docs/hover-comparison.svg').write_text(ET.tostring(svg,encoding='unicode'))
