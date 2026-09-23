#!/usr/bin/env python3
"""Verify exported scope data and note timing against the exact recorded WAV."""
import array,json,math,sys,wave
from pathlib import Path
p=Path(sys.argv[1]);text=(p/'assets/take.mjs').read_text();take=json.loads(text.removeprefix('export default ').strip().removesuffix(';'))
with wave.open(str(p/'assets/audio.wav'),'rb') as wav:
 assert (wav.getframerate(),wav.getnchannels(),wav.getsampwidth())==(48000,2,2)
 samples=array.array('h');samples.frombytes(wav.readframes(wav.getnframes()))
 if sys.byteorder!='little':samples.byteswap()
assert max(map(abs,samples))>100,'silent recording'
assert abs(len(samples)/96000-take['duration'])<1/48000
for frame,scope in enumerate(take['scopes']):
 end=min(len(samples)//2,round(frame*48000/30))
 expected=[0 if (n:=end-2048+i*8)<0 else round((samples[2*n]+samples[2*n+1])/2) for i in range(256)]
 assert scope==expected,f'scope {frame} disagrees with the WAV'
notes=[e for e in take['events'] if e['type']=='note' and e.get('command',{}).get('op')=='note_on']
assert {69,71,72}<={e['command']['note'] for e in notes},'missing A/B/C proof'
assert all((e['frame']%480)==0 for e in notes),'notes must be acknowledged on DSP block boundaries'
scenes=[e for e in take['events'] if e['type']=='scene'];assert len(scenes)>=2
assert all((p/'assets'/f'scene-{e["revision"]}'/'scene.json').is_file() for e in scenes)
print(f'PASS: {take["duration"]:.2f}s native audio; all {len(take["scopes"])} scope windows exactly match WAV; A/B/C sample timing and {len(scenes)} scene revisions.')
