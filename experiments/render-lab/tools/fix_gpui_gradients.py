"""Apply the pinned wgpu gradient transfer fix shared by the lab and plugin.

Hsla carries sRGB channels and the renderer uses a non-sRGB attachment.
Match the pinned Metal gradient convention; leave solid colors/blending alone.
"""
from pathlib import Path

p = Path(__file__).resolve().parents[2] / "upstream/crates/gpui_wgpu/src/shaders.wgsl"
s = p.read_text()
marker = "// MUI: gradient stops and Unorm output are sRGB encoded."
if marker not in s:
    old = """        // The hsla_to_rgba is returns a linear sRGB color
        result.color0 = hsla_to_rgba(colors[0].color);
        result.color1 = hsla_to_rgba(colors[1].color);

        // Prepare color space in vertex for avoid conversion
        // in fragment shader for performance reasons
        if (color_space == 0u) {
            // sRGB
            result.color0 = linear_to_srgba(result.color0);
            result.color1 = linear_to_srgba(result.color1);
        } else if (color_space == 1u) {
            // Oklab
            result.color0 = linear_srgb_to_oklab(result.color0);
            result.color1 = linear_srgb_to_oklab(result.color1);
        }"""
    new = """        // MUI: gradient stops and Unorm output are sRGB encoded.
        result.color0 = hsla_to_rgba(colors[0].color);
        result.color1 = hsla_to_rgba(colors[1].color);
        if (color_space == 1u) {
            result.color0 = linear_srgb_to_oklab(srgba_to_linear(result.color0));
            result.color1 = linear_srgb_to_oklab(srgba_to_linear(result.color1));
        }"""
    replacements = [
        (old, new),
        ("background_color = srgba_to_linear(mix(color0, color1, t));",
         "background_color = mix(color0, color1, t);"),
        ("background_color = oklab_to_linear_srgb(oklab_color);",
         "background_color = linear_to_srgba(oklab_to_linear_srgb(oklab_color));"),
    ]
    for old, new in replacements:
        assert s.count(old) == 1, f"Pinned gradient source changed: {old[:70]}"
        s = s.replace(old, new)
    p.write_text(s)
print("GPUI wgpu gradient transfer fix applied (or already present).")
