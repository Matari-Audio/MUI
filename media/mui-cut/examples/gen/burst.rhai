// 400 sparks burst from the centre, curve out on noise and fade; a title
// punches in over them. `mui-cut gen burst.rhai -o burst.cut.json --seed 3`
let W = 1920.0; let H = 1080.0;
let layers = [];
for i in 0..400 {
    let a = rand(0.0, 2.0 * PI());
    let d = rand(200.0, 900.0);
    let t0 = rand(0.0, 0.4);
    let life = rand(0.9, 1.8);
    let wob = noise(i * 0.1, 0.5) * 160.0;
    let x1 = W / 2.0 + a.cos() * d - a.sin() * wob;
    let y1 = H / 2.0 + a.sin() * d + a.cos() * wob;
    let size = rand(4.0, 14.0);
    layers.push(#{
        id: `spark${i}`, kind: "ellipse", width: size, height: size,
        fill: hsl(20.0 + rand(0.0, 50.0), 1.0, rand(0.5, 0.75)),
        x: [key(t0, W / 2.0), #{ t: t0 + life, v: x1, out: [0.0, 0.0], "in": [-life * 0.7, 0.0] }],
        y: [key(t0, H / 2.0), #{ t: t0 + life, v: y1, "in": [-life * 0.7, 0.0] }],
        opacity: [key(t0, 0.0, "linear"), key(t0 + 0.05, 1.0), key(t0 + life, 0.0)],
    });
}
layers.push(#{
    id: "title", kind: "text", text: "BURST", x: W / 2.0, y: H / 2.0,
    font_size: 220, weight: 900, fill: "#fff4e0",
    scale: [key(0.2, 0.0), #{ t: 0.6, v: 1.0, "in": [-0.15, 0.25] }],
    animators: [#{ by: "char", stagger: 0.04, amount: [key(0.2, 1.0), key(0.8, 0.0)], y: 80, opacity: 0 }],
});
#{ size: [1920, 1080], fps: 30, scenes: [#{ name: "burst", duration: 3, background: "#0b0710", layers: layers }] }
