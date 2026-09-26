"""Freeze one sample-clocked take into a standalone HyperFrames composition."""
import array, json, math, os, re, shutil, wave
from pathlib import Path

def export_film(session,record,root):
    folder=session/('film-'+record['id'] if 'id' in record else 'film');assets=folder/'assets';assets.mkdir(parents=True,exist_ok=True)
    pcm=array.array('f');pcm.frombytes(b''.join(record['pcm']))
    if __import__('sys').byteorder!='little':pcm.byteswap()
    quantized=array.array('h',(max(-32767,min(32767,round(x*32767))) for x in pcm))
    duration=len(quantized)/2/48000
    with wave.open(str(assets/'audio.wav'),'wb') as wav:
        wav.setnchannels(2);wav.setsampwidth(2);wav.setframerate(48000)
        if __import__('sys').byteorder!='little':quantized.byteswap()
        wav.writeframes(quantized.tobytes())
        if __import__('sys').byteorder!='little':quantized.byteswap()
    events=sorted(record['events'],key=lambda e:e['frame'])
    revisions=sorted({e['revision'] for e in events if e['type']=='scene'})
    textures=assets/'textures';textures.mkdir(exist_ok=True)
    for revision in revisions:
        source=session/f'scene-{revision}';dest=assets/source.name;dest.mkdir(exist_ok=True)
        shutil.copyfile(source/'scene.json',dest/'scene.json')
        for image in source.glob('*.png'):
            target=(textures if re.fullmatch(r'[a-f0-9]{64}\.png',image.name) else dest)/image.name
            if not target.exists():os.link(image,target)
    # Windows end at the displayed sample, matching an output oscilloscope's
    # recent history. Derive from the exact PCM16 WAV, never an oscillator model.
    scopes=[]
    for f in range(math.ceil(duration*30)+1):
        end=min(len(quantized)//2,round(f*48000/30));window=[]
        for i in range(256):
            n=end-2048+i*8
            window.append(0 if n<0 else round((quantized[2*n]+quantized[2*n+1])/2))
        scopes.append(window)
    data={'version':1,'plugin':record['plugin'],'sampleRate':48000,'start':record['start'],'duration':duration,'events':events,'scopes':scopes}
    (assets/'take.mjs').write_text('export default '+json.dumps(data,separators=(',',':'))+';\n')
    (folder/'take.json').write_text(json.dumps({k:v for k,v in data.items() if k!='scopes'},indent=2)+'\n')
    for name in ['Inter-V.otf','Inter-LICENSE.txt']:shutil.copyfile(root/'tools/film/assets'/name,assets/name)
    shutil.copyfile(root/'tools/film/layers.mjs',assets/'layers.mjs')
    template=(root/'tools/film/film.html').read_text().replace('__DURATION__',str(duration))
    (folder/'index.html').write_text(template)
    (folder/'package.json').write_text(json.dumps({'private':True,'type':'module','scripts':{'dev':'npx --yes hyperframes@0.8.66 preview','check':'npx --yes hyperframes@0.8.66 check','render':'npx --yes hyperframes@0.8.66 render'}},indent=2)+'\n')
    (folder/'hyperframes.json').write_text('{}\n')
    (folder/'BRIEF.md').write_text('workflow: general-video\nflow: companion\n\nRecorded MUI instrument proof. Native PCM audio, note states, native scene revisions, and presentation poses share a sample clock. The scope derives from the exported WAV. No synthesis occurs at video-render time.\n')
    (folder/'assets/PROVENANCE.md').write_text('Audio: native plugin PCM captured by mui-motion-bridge, quantized to stereo PCM16 at48kHz. Scope: exact exported WAV. UI: native MUI scene capture from the same session. Fonts retain the bundled upstream license; GSAP loads from a pinned CDN URL under its own license.\n')
    return folder
