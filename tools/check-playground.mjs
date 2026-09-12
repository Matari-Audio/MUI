import fs from 'node:fs';
import assert from 'node:assert/strict';
import {initSync, render, parameters, hover, tap} from '../playground/pkg/mui_playground.js';
globalThis.document={createElement:()=>({getContext:()=>({measureText:text=>({width:[...text].length*8})})})};
initSync({module:fs.readFileSync(new URL('../playground/pkg/mui_playground_bg.wasm', import.meta.url))});
const code=fs.readFileSync(new URL('../playground/default.mui', import.meta.url),'utf8');
const svg=render(code,420,true,18,0x7864dc,true);
assert(svg.includes('Filter'));assert(parameters(code).includes('round\t20\ttrue'));
assert.notEqual(render(code,420,false,18,0x7864dc,true),svg);
render(code,420,true,18,0x7864dc,true);
let hit;
for(let y=0;y<70&&!hit;y+=2)for(let x=0;x<420;x+=2)if(tap(x,y)==='filter'){hit=[x,y];break;}
assert(hit);assert(hover(...hit,true).includes('data-hover="filter"'));
assert.notEqual(hover(...hit,true),svg);
assert.throws(()=>render('fetch("evil")',420,true,18,0x7864dc,true));
assert.equal(tap(...hit),'filter');
console.log('WASM integration passed: default layout, sliders, light/dark, hover, tap, invalid-source rollback. Text measurement stubbed; browser UI not tested.');
