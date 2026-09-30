//! `mui-cut midi`: a Standard MIDI File's notes as a plugin layer's
//! `notes`, in seconds through the file's tempo map.
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use mui_cut::Note;

use crate::Result;

/// Every note in `bytes` (or in `track` alone), `at` seconds later, by start.
pub fn notes(bytes: &[u8], track: Option<usize>, at: f64) -> Result<Vec<Note>> {
    let smf = Smf::parse(bytes).map_err(|e| format!("not a MIDI file: {e}"))?;
    // (tick, event) for every track, absolute ticks.
    let mut tempo: Vec<(u64, f64)> = Vec::new(); // (tick, µs per beat)
    let mut events: Vec<(u64, usize, bool, u8, u8)> = Vec::new(); // (tick, track, on, key, vel)
    for (i, t) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        for e in t {
            tick += u64::from(e.delta.as_int());
            match e.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => tempo.push((tick, f64::from(us.as_int()))),
                TrackEventKind::Midi { message, .. } if track.is_none_or(|n| n == i) => match message {
                    MidiMessage::NoteOn { key, vel } if vel > 0 => events.push((tick, i, true, key.as_int(), vel.as_int())),
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        events.push((tick, i, false, key.as_int(), 0));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if let Some(n) = track.filter(|n| *n >= smf.tracks.len()) {
        return Err(format!("--track {n}: the file has {} tracks", smf.tracks.len()));
    }
    tempo.sort_by_key(|t| t.0);
    let seconds: Box<dyn Fn(u64) -> f64> = match smf.header.timing {
        Timing::Metrical(tpb) => {
            let tpb = f64::from(tpb.as_int().max(1));
            Box::new(move |tick| {
                // 120 bpm until the first tempo event.
                let (mut s, mut last, mut us) = (0., 0u64, 500_000.);
                for &(t, u) in tempo.iter().take_while(|(t, _)| *t <= tick) {
                    s += (t - last) as f64 / tpb * us / 1e6;
                    (last, us) = (t, u);
                }
                s + (tick - last) as f64 / tpb * us / 1e6
            })
        }
        Timing::Timecode(fps, sub) => {
            let per = f64::from(fps.as_f32()) * f64::from(sub);
            Box::new(move |tick| tick as f64 / per)
        }
    };
    events.sort_by_key(|e| (e.0, e.2));
    let mut held: Vec<(usize, u8, u64, u8)> = Vec::new();
    let mut out = Vec::new();
    for (tick, tr, on, key, vel) in events {
        // A retrigger or an off ends the key's held note.
        if let Some(i) = held.iter().position(|h| h.0 == tr && h.1 == key) {
            let (_, k, from, v) = held.remove(i);
            let t = seconds(from);
            let dur = seconds(tick) - t;
            if dur > 0. {
                out.push(Note { t: t + at, dur, pitch: k, vel: v.max(1) });
            }
        }
        if on {
            held.push((tr, key, tick, vel));
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.pitch.cmp(&b.pitch)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    /// A one-track file at 60 bpm (a second a beat), 96 ticks a beat:
    /// C4 for a beat, then E4 for half a beat after a half-beat rest.
    #[test]
    fn notes_come_out_in_seconds_through_the_tempo_map() {
        let track: Vec<u8> = [
            &[0x00, 0xFF, 0x51, 0x03, 0x0F, 0x42, 0x40][..], // tempo 1,000,000 µs
            &[0x00, 0x90, 60, 100],
            &[0x60, 0x80, 60, 0],
            &[0x30, 0x90, 64, 80],
            &[0x30, 0x90, 64, 0], // note on at velocity 0 is an off
            &[0x00, 0xFF, 0x2F, 0x00],
        ]
        .concat();
        let mut file = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk".to_vec();
        file.extend((track.len() as u32).to_be_bytes());
        file.extend(track);
        let n = super::notes(&file, None, 2.).unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].t, n[0].dur, n[0].pitch, n[0].vel), (2., 1., 60, 100));
        assert_eq!((n[1].t, n[1].dur, n[1].pitch, n[1].vel), (3.5, 0.5, 64, 80));
    }
}
