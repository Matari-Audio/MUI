// <MuiReel dir="gain-reel" /> -- a mui-reel take in a Remotion composition.
// Copy this file into your Remotion src/ and the reel's output folder into
// public/<dir>/. No build step, no dependency beyond `remotion`.
//
//   <MuiReel dir="gain-reel">
//     <Callout />            // children read the track with useMuiTrack()
//   </MuiReel>
//
//   const {rect, pointer, beat} = useMuiTrack();
//   const r = rect("gain");  // {x, y, w, h} in take pixels at this frame, or null
import React, {createContext, useContext, useEffect, useState} from 'react';
import {AbsoluteFill, Audio, OffthreadVideo, cancelRender, continueRender, delayRender, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';

type Row = (number | boolean)[] | null;
type Track = {fps: number; frames: number; bpm: number | null; surfaces: Record<string, Row[]>; pointer: Row[]; camera: Row[]};
const Ctx = createContext<Track | null>(null);

export const MuiReel: React.FC<{dir: string; audio?: boolean; children?: React.ReactNode}> = ({dir, audio = true, children}) => {
	const [track, setTrack] = useState<Track | null>(null);
	const [handle] = useState(() => delayRender(`mui-reel track ${dir}`));
	useEffect(() => {
		fetch(staticFile(`${dir}/track.json`))
			.then((r) => r.json())
			.then((t) => { setTrack(t); continueRender(handle); })
			.catch(cancelRender);
	}, [dir, handle]);
	return (
		<AbsoluteFill>
			<OffthreadVideo src={staticFile(`${dir}/take.mp4`)} muted />
			{audio ? <Audio src={staticFile(`${dir}/audio.wav`)} /> : null}
			{track ? <Ctx.Provider value={track}>{children}</Ctx.Provider> : null}
		</AbsoluteFill>
	);
};

// Linear between the take's two frames around this composition frame, so a
// 30 fps composition over a 60 fps take (or the reverse) stays exact.
const at = (rows: Row[], t: Track, sec: number): number[] | null => {
	const f = Math.min(t.frames - 1, Math.max(0, sec * t.fps));
	const i = Math.floor(f), j = Math.min(i + 1, t.frames - 1), u = f - i;
	const a = rows[i], b = rows[j];
	if (!a) return null;
	return a.map((v, k) => (typeof v === 'number' && b ? v + ((b[k] as number) - v) * u : Number(v)));
};

export const useMuiTrack = () => {
	const t = useContext(Ctx);
	const sec = useCurrentFrame() / useVideoConfig().fps;
	if (!t) throw new Error('useMuiTrack() outside <MuiReel>');
	return {
		rect: (id: string) => { const r = t.surfaces[id] && at(t.surfaces[id], t, sec); return r && {x: r[0], y: r[1], w: r[2], h: r[3]}; },
		pointer: () => { const p = at(t.pointer, t, sec); return p && {x: p[0], y: p[1], down: p[2] === 1}; },
		camera: () => { const c = at(t.camera, t, sec)!; return {x: c[0], y: c[1], zoom: c[2]}; },
		beat: (n: number) => (n * 60) / (t.bpm ?? 120),
	};
};
