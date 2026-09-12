import { test } from "node:test";
import assert from "node:assert/strict";
import {compile} from "../src/compiler.js";
import {defineScene,leaf,row,flow,frameSurface,mergeSurface,extendTo,space} from "../src/index.js";

test("Rust literals round-trip controls, Unicode, exponents and negative zero",()=>{
  const source=compile(defineScene({root:leaf('a\u0001b\b\f\\"שלום🎹',[-0,10]),surfaces:[],theme:{corners:{convex:1e21,concave:0}}}));
  assert.ok(source.includes('a\\u{1}b\\u{8}\\u{c}'));assert.ok(source.includes('שלום🎹'));
  assert.ok(source.includes('1e+21f64'));assert.ok(source.includes('-0f64'));assert.ok(!source.includes('1e+21.0'));
  assert.throws(()=>compile({root:leaf('\ud800',[1,1]),surfaces:[]}),/surrogate/);
});
test("invalid references, numbers and merges fail before Rust emission",()=>{
  for(const scene of [
    {root:leaf("r",[1,1]),surfaces:[mergeSurface("m",[])]},
    {root:row("r",[leaf("x",[1,1]),leaf("x",[1,1])]),surfaces:[]},
    {root:leaf("r",[NaN,1]),surfaces:[]},
    {root:leaf("r",[1,1]),surfaces:[frameSurface("s","missing")]},
    {root:leaf("r",[1,1]),surfaces:[extendTo(frameSurface("s","r"),"bottom","missing")]},
  ]) assert.throws(()=>compile(scene),/MUI:/);
});
test("compact responsive scenes emit tokens, extensions and palettes deterministically",()=>{
  const scene=defineScene({root:flow([leaf("tab",[80,32]),leaf("panel",[320,100])],{axis:"column",gap:space.m,padding:space.s,width:"fill",height:"hug"}),surfaces:[extendTo(frameSurface("tab-s","tab"),"bottom","panel"),frameSurface("panel-s","panel"),mergeSurface("shell",["tab-s","panel-s"])],theme:{mode:"dark",colors:{primary:[0xaa22cc,0x44ccaa,0xdd5522],neutral:0x888888}}});
  const code=compile(scene);assert.equal(code,compile(scene));
  for(const token of [".extend_to(mui_core::Edge::Bottom", "mui_core::Spacing::m()", "Sizing::Fill", "Mode::Dark", "Rgb::new(170, 34, 204)"]) assert.ok(code.includes(token),token);
});
