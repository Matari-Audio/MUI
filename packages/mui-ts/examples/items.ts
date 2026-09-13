import { container, item, grid, space, defineUi } from "../src/index.js";

export default defineUi({
  availableWidth: 360,
  root: container([
    container([
      item("osc").text("Oscillator").pad(space.s).onTap("select-osc"),
      item("filter").text("Filter").pad(space.s).extendTo("panel").color("raised").onTap("select-filter"),
      item("fx").text("Effects").pad(space.s).onTap("select-fx"),
    ]).layout("row").center().gap(space.s).pad(space.m).width("fill"),
    item("panel").layout(grid(3)).pad(space.l).gap(space.m).width("fill").children([
      item("cutoff").width(64).height(64),
      item("resonance").width(64).height(64),
      item("drive").width(64).height(64),
    ]),
  ]).layout("column").width("fill").height("hug").gap(space.m)
    .round({ outer: space.m, inner: space.s }).merge(["filter", "panel"]),
});
