use super::pool::{bucket, ContentState};
use mui_weld::analytic::{dirty_ranges, PARAM_BYTES};
#[test]
fn aborted_encoder_does_not_mark_texture_rendered() {
    let a = [1; PARAM_BYTES];
    let b = [2; PARAM_BYTES];
    let mut state = ContentState {
        uploaded: Some(a),
        rendered: Some(a),
        pending: None,
    };
    state.uploaded = Some(b);
    state.pending = Some(b);
    state.aborted();
    assert!(state.needs_render(&b));
    assert_eq!(dirty_ranges(state.uploaded.as_ref(), &b).count(), 0);
}
#[test]
fn submission_commits_but_does_not_claim_gpu_completion() {
    let p = [3; PARAM_BYTES];
    let mut s = ContentState {
        pending: Some(p),
        ..Default::default()
    };
    assert!(s.needs_render(&p));
    s.submitted();
    assert!(!s.needs_render(&p));
    assert!(s.pending.is_none());
}
#[test]
fn return_to_old_value_after_abort_reuses_the_old_texture() {
    let a = [1; PARAM_BYTES];
    let mut s = ContentState {
        rendered: Some(a),
        uploaded: Some([2; PARAM_BYTES]),
        pending: Some([2; PARAM_BYTES]),
    };
    s.aborted();
    assert!(!s.needs_render(&a));
    assert_eq!(
        dirty_ranges(s.uploaded.as_ref(), &a).collect::<Vec<_>>(),
        vec![0..PARAM_BYTES]
    );
}
#[test]
fn size_buckets_are_bounded_and_cover_request() {
    for v in 1..4097 {
        let b = bucket([v, v], 4096);
        assert!(b[0] >= v && b[0] <= 4096);
        assert_eq!(b[0] % 64, 0);
    }
}
#[test]
fn non_multiple_device_limit_does_not_overflow() {
    assert_eq!(bucket([65535, 1], 65535), [65535, 64]);
}
#[test]
fn sparse_dirty_ranges_are_coalesced_without_painting_everything() {
    let old = [0; PARAM_BYTES];
    let mut new = old;
    new[16] = 1;
    new[32] = 2;
    new[80] = 3;
    assert_eq!(
        dirty_ranges(Some(&old), &new).collect::<Vec<_>>(),
        vec![16..48, 80..96]
    );
}
#[test]
fn production_shader_has_both_explicit_alpha_contracts() {
    assert!(super::WELD_SHADER.contains("fn fs_hybrid"));
    assert!(super::WELD_SHADER.contains("fn fs_classic"));
    assert!(super::WELD_SHADER.contains("array<Source, 3>"));
}

/// This is the real WGSL parser/type/uniformity check, not a string assertion.
/// It runs without an adapter; native pixel tests are a separate opt-in binary.
#[test]
fn naga_validates_both_outputs_and_the_uniform_abi() {
    let module = naga::front::wgsl::parse_str(super::WELD_SHADER)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::WELD_SHADER)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("WGSL type/uniformity validation");
    for name in ["vs_main", "fs_hybrid", "fs_classic"] {
        assert!(module.entry_points.iter().any(|e| e.name == name));
    }
    let uniform = module
        .global_variables
        .iter()
        .map(|(_, v)| v)
        .find(|v| v.name.as_deref() == Some("u"))
        .expect("u");
    let naga::TypeInner::Struct { members, span } = &module.types[uniform.ty].inner else {
        panic!("Params must be a struct")
    };
    assert_eq!(*span, PARAM_BYTES as u32);
    assert_eq!(
        members.iter().map(|m| m.offset).collect::<Vec<_>>(),
        [0, 16, 32, 48, 64]
    );
    let naga::TypeInner::Array { stride, .. } = &module.types[members[4].ty].inner else {
        panic!("sources must be an array")
    };
    assert_eq!(*stride, 96);
}

#[test]
fn naga_validates_crisp_boundary_uniform_stride() {
    let m = naga::front::wgsl::parse_str(super::WELD_SHADER).expect("WGSL parse");
    let b = m
        .global_variables
        .iter()
        .map(|(_, v)| v)
        .find(|v| v.name.as_deref() == Some("boundary"))
        .expect("boundary binding");
    assert_eq!(b.binding.as_ref().unwrap().binding, 1);
    let naga::TypeInner::Struct { members, span } = &m.types[b.ty].inner else {
        panic!("boundary struct")
    };
    assert_eq!(*span, mui_weld::boundary::BOUNDARY_BYTES as u32);
    let naga::TypeInner::Array { stride, .. } = &m.types[members[0].ty].inner else {
        panic!("edges")
    };
    assert_eq!(*stride, 48);
}
