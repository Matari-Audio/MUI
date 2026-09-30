// A 16 x 9 grid of tiles that flips in as a diagonal wave and breathes
// with Perlin noise: 144 layers, 5 keys each.
let cols = 16; let rows = 9; let cell = 110.0;
let layers = [];
for r in 0..rows {
    for c in 0..cols {
        let delay = (c + r) * 0.04;
        let n = noise(c * 0.3, r * 0.3);
        layers.push(#{
            id: `tile_${c}_${r}`, kind: "rect", width: cell - 14.0, height: cell - 14.0, radius: 14,
            x: 960.0 + (c - (cols - 1) / 2.0) * cell,
            y: 540.0 + (r - (rows - 1) / 2.0) * cell,
            fill: hsl(200.0 + n * 90.0, 0.8, 0.55),
            rotation: [key(delay, -90.0), #{ t: delay + 0.6, v: 0.0, "in": [-0.3, 0.0] }],
            scale: [key(delay, 0.0), key(delay + 0.5, 1.0), key(2.0 + delay, 1.0), key(2.4 + delay, 0.6 + n * 0.3), key(3.5, 1.0)],
        });
    }
}
#{ size: [1920, 1080], fps: 30, scenes: [#{ name: "grid", duration: 3.5, background: "#0d1020", layers: layers }] }
