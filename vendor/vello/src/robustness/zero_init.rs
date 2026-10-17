// Copyright 2026 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Explicit DX12 workgroup clearing avoids Naga's HLSL aggregate assignments.
//! The locked shaders use scalars, structs and fixed arrays (including atomics).
use std::borrow::Cow;
use std::fmt::Write as _;

pub(super) fn prepare(source: Cow<'_, str>) -> Cow<'_, str> {
    let variables: Vec<_> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("var<workgroup> "))
        .map(|line| {
            let (name, ty) = line.split_once(':').expect("workgroup declaration changed");
            (
                name.trim(),
                ty.trim().strip_suffix(';').expect("workgroup type changed"),
            )
        })
        .collect();
    assert_eq!(variables.len(), source.matches("var<workgroup>").count());
    if variables.is_empty() {
        return source;
    }
    assert!(!source.contains("mui_zero_"), "initializer name collision");
    let main = source.find("fn main(").expect("shader entry point changed");
    let arguments = main + "fn main(".len();
    let body = arguments
        + source[arguments..]
            .find(") {")
            .expect("shader entry point body changed")
        + ") {".len();
    let size = source[..main]
        .rfind("@workgroup_size(")
        .expect("workgroup size missing")
        + "@workgroup_size(".len();
    let count = source[size..]
        .split_once(')')
        .expect("workgroup size changed")
        .0
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" * ");
    let builtin = "@builtin(local_invocation_index)";
    let existing = source[arguments..body].find(builtin).map(|position| {
        source[arguments + position + builtin.len()..]
            .split_once(':')
            .expect("local invocation index changed")
            .0
            .trim()
    });
    let lane = existing.unwrap_or("mui_zero_lane");
    let mut initialization =
        String::from("\n    // MUI: clear every workgroup value before any shader work.\n");
    for (name, ty) in variables {
        clear(&mut initialization, name, ty, 0, Some((lane, &count)));
    }
    initialization.push_str("    workgroupBarrier();\n");
    let add_argument = existing.is_none();
    let mut result = source.into_owned();
    result.insert_str(body, &initialization);
    if add_argument {
        result.insert_str(
            arguments,
            "\n    @builtin(local_invocation_index) mui_zero_lane: u32,",
        );
    }
    Cow::Owned(result)
}

fn clear(out: &mut String, name: &str, ty: &str, depth: usize, lanes: Option<(&str, &str)>) {
    let ty = ty.trim();
    if let Some(array) = ty
        .strip_prefix("array<")
        .and_then(|ty| ty.strip_suffix('>'))
    {
        let mut nesting = 0;
        let comma = array
            .char_indices()
            .find_map(|(position, character)| match character {
                '<' => {
                    nesting += 1;
                    None
                }
                '>' => {
                    nesting -= 1;
                    None
                }
                ',' if nesting == 0 => Some(position),
                _ => None,
            })
            .expect("fixed workgroup array changed");
        let element = array[..comma].trim();
        let length = array[comma + 1..].trim();
        let (start, stride) = lanes.unwrap_or(("0u", "1u"));
        writeln!(out, "    for (var mui_zero_i{depth} = {start}; mui_zero_i{depth} < u32({length}); mui_zero_i{depth} += u32({stride})) {{").unwrap();
        clear(
            out,
            &format!("{name}[mui_zero_i{depth}]"),
            element,
            depth + 1,
            None,
        );
        out.push_str("    }\n");
    } else if let Some((lane, _)) = lanes {
        writeln!(out, "    if ({lane} == 0u) {{").unwrap();
        clear(out, name, ty, depth, None);
        out.push_str("    }\n");
    } else if ty.starts_with("atomic<") {
        let zero = if ty == "atomic<i32>" { "0i" } else { "0u" };
        writeln!(out, "    atomicStore(&{name}, {zero});").unwrap();
    } else {
        writeln!(out, "    {name} = {ty}();").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_shader_accepts_explicit_workgroup_initialization() {
        let s = vello_shaders::SHADERS;
        for shader in [
            s.pathtag_reduce,
            s.pathtag_reduce2,
            s.pathtag_scan1,
            s.pathtag_scan_small,
            s.pathtag_scan_large,
            s.bbox_clear,
            s.flatten,
            s.draw_reduce,
            s.draw_leaf,
            s.clip_reduce,
            s.clip_leaf,
            s.binning,
            s.tile_alloc,
            s.backdrop_dyn,
            s.path_count_setup,
            s.path_count,
            s.coarse,
            s.path_tiling_setup,
            s.path_tiling,
            s.fine_area,
            s.fine_msaa8,
            s.fine_msaa16,
        ] {
            let source = prepare(shader.wgsl.code.clone());
            let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
                panic!("{}: {}", shader.name, error.emit_to_string(&source))
            });
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{}: {error:?}", shader.name));
            if !shader.workgroup_buffers.is_empty() {
                let clear = source
                    .split("// MUI: clear every workgroup value")
                    .nth(1)
                    .expect("missing initialization")
                    .split("workgroupBarrier();")
                    .next()
                    .unwrap();
                for (_, variable) in module.global_variables.iter() {
                    if variable.space == wgpu::naga::AddressSpace::WorkGroup {
                        assert!(
                            clear.contains(variable.name.as_deref().unwrap()),
                            "{}: missing {variable:?}",
                            shader.name
                        );
                    }
                }
            }
        }
    }
}
