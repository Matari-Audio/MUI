use super::pool::{ContentState, bucket};
use mui_weld::analytic::{PARAM_BYTES, dirty_ranges};
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
fn weld_uniform_layout_matches_the_host() {
    let shader = mui_shaders::ShaderId::Weld.artifact();
    for name in ["vs_main", "fs_hybrid"] {
        assert!(shader.entry_point(name).is_some(), "missing {name}");
    }

    let layout = shader.binding("u").unwrap().uniform_layout.unwrap();
    assert_eq!(layout.size, PARAM_BYTES as u32);
    assert_eq!(
        layout.members.iter().map(|m| m.offset).collect::<Vec<_>>(),
        [0, 16, 32, 48]
    );
    assert_eq!(layout.members[3].array_stride, Some(96));

    let boundary = shader.binding("boundary").unwrap();
    assert_eq!(boundary.binding, 1);
    let layout = boundary.uniform_layout.unwrap();
    assert_eq!(layout.size, mui_weld::boundary::BOUNDARY_BYTES as u32);
    assert_eq!(layout.members[0].array_stride, Some(48));
}
