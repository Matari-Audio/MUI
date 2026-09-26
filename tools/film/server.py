#!/usr/bin/env python3
"""Loopback-only development gateway for any mui-motion-bridge adapter executable."""
import argparse, asyncio, base64, contextlib, json, os, re, struct, sys, uuid
from pathlib import Path
from concurrent.futures import ProcessPoolExecutor
from aiohttp import web, WSMsgType
from export import export_film, inter
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent

class Gateway:
    def __init__(self,binary,sessions,port):
        self.binary=binary;self.sessions=sessions;self.port=port;self.busy=False
        # WAV conversion and large JSON encoding must not hold the audio pump's GIL.
        self.exports=ProcessPoolExecutor(max_workers=1)
    async def socket(self,request):
        origin=request.headers.get('Origin')
        if origin and origin not in (f'http://localhost:{self.port}',f'http://127.0.0.1:{self.port}'):
            raise web.HTTPForbidden(text='Use the local MUI Motion page.')
        if self.busy:raise web.HTTPConflict(text='One instrument session is already open.')
        self.busy=True
        ws=web.WebSocketResponse(max_msg_size=65536,heartbeat=20);await ws.prepare(request)
        session=self.sessions/uuid.uuid4().hex;session.mkdir(parents=True)
        try:
            with (session/'native.log').open('wb') as log:
                proc=await asyncio.create_subprocess_exec(str(self.binary),str(session),stdin=asyncio.subprocess.PIPE,stdout=asyncio.subprocess.PIPE,stderr=log)
        except OSError:
            self.busy=False
            await ws.close()
            raise
        state={'frame':0,'scene':None,'plugin':None,'active':[],'record':None,'pose':{'explode':0,'angle':0,'poses':{}},'takes':{},'renders':set(),'images':{}}
        async def native():
            while True:
                header=await proc.stdout.readexactly(5);kind=header[0];size=struct.unpack('<I',header[1:])[0]
                if size>32*1024*1024:raise ValueError('native packet exceeds limit')
                payload=await proc.stdout.readexactly(size)
                if kind==ord('A'):
                    frame=struct.unpack('<Q',payload[:8])[0];frames=(len(payload)-8)//8;state['frame']=frame+frames
                    record=state['record']
                    if record:
                        if frame-record['start']<120*48000:record['pcm'].append(payload[8:]);record['end']=frame+frames
                        else:asyncio.create_task(stop_record())
                    await ws.send_bytes(payload)
                elif kind==ord('J'):
                    value=json.loads(payload)
                    if value['type']=='hello':state['plugin']=value['plugin']
                    if value['type']=='note':state['active']=value['active']
                    if value['type']=='scene':
                        if 'images' in value:
                            for name,data in value['images'].items():
                                if not re.fullmatch(r'[a-f0-9]{64}\.png',name):raise ValueError('Invalid texture name')
                                state['images'][name]=data
                            names={layer['src'] for layer in value['scene']['layers']}
                            state['images']={name:state['images'][name] for name in names}
                            # Frames stay in memory during preview. Disk is only a recording sink.
                            if state['record']:await asyncio.to_thread(save_frame,value,state['images'])
                        state['scene']={k:v for k,v in value.items() if k!='images'}
                        value['assetBase']=f'/sessions/{session.name}/scene-{value["revision"]}/'

                    if state['record'] and value['type'] in ('note','edit','scene'):state['record']['events'].append({k:v for k,v in value.items() if k!='images'})
                    await ws.send_json(value)
                else:raise ValueError('unknown native packet')
        def save_frame(value,images):
            folder=session/f'scene-{value["revision"]}';folder.mkdir(exist_ok=True)
            cache=session/'textures';cache.mkdir(exist_ok=True)
            for layer in value['scene']['layers']:
                name=layer['src'];file=cache/name
                if not file.exists():file.write_bytes(base64.b64decode(images[name],validate=True))
                target=folder/name
                if not target.exists():os.link(file,target)
            (folder/'scene.json').write_text(json.dumps(value['scene']))
        async def stop_record():
            record=state['record'];state['record']=None
            if not record:return
            if not record['pcm']:
                await ws.send_json({'type':'error','message':'Recording was empty.'});return
            record['plugin']=state['plugin']
            try:folder=await asyncio.get_running_loop().run_in_executor(self.exports,export_film,session,record,ROOT)
            except OSError:
                # Keep the in-memory take available for another stop attempt.
                state['record']=record
                raise
            state['takes'][folder.name]=folder
            await ws.send_json({'type':'recorded','take':folder.name,'project':str(folder),'url':f'/sessions/{session.name}/{folder.name}/index.html','seconds':(record['end']-record['start'])/48000})
        async def pump():
            try:await native()
            except (asyncio.IncompleteReadError,ValueError,OSError) as error:
                if not ws.closed:
                    await ws.send_json({'type':'error','message':'Native adapter stopped. Check the session native.log: '+str(error)})
                    await ws.close()
        task=asyncio.create_task(pump())
        try:
            async for message in ws:
                if message.type!=WSMsgType.TEXT:continue
                try:
                    command=json.loads(message.data)
                    if not isinstance(command,dict):raise ValueError('Expected an object.')
                    op=command.get('op')
                    if op=='record_start':
                        if state['record']:raise ValueError('Already recording.')
                        if not state['scene']:raise ValueError('Wait for the editor to load.')
                        start=state['frame'];initial=dict(state['scene'],frame=start)
                        if state['images']:save_frame(initial,state['images'])
                        pose={'type':'pose','frame':start,'pose':state['pose']}
                        state['record']={'id':uuid.uuid4().hex[:12],'start':start,'end':start,'pcm':[],'events':[initial,pose,{'type':'note','frame':start,'active':list(state['active'])}]}
                        await ws.send_json({'type':'recording','frame':start})
                    elif op=='record_stop':await stop_record()
                    elif op=='render':
                        name=command.get('take');folder=state['takes'].get(name)
                        if folder is None:raise ValueError('Record a take first.')
                        if name in state['renders']:raise ValueError('This take is already rendering.')
                        state['renders'].add(name)
                        async def render_take(name=name,folder=folder):
                            try:
                                await ws.send_json({'type':'rendering','take':name})
                                with (folder/'render.log').open('wb') as log:
                                    job=await asyncio.create_subprocess_exec(sys.executable,str(HERE/'render.py'),str(folder),stdout=log,stderr=log)
                                    code=await job.wait()
                                if not ws.closed:
                                    await ws.send_json({'type':'rendered','take':name,'url':f'/sessions/{session.name}/{name}/performance.mp4'} if code==0 else {'type':'error','message':'Render failed; see '+str(folder/'render.log')})
                            finally:state['renders'].discard(name)
                        asyncio.create_task(render_take())
                    elif op=='pose':
                        pose=command.get('pose');validate_pose(pose);state['pose']=pose
                        if state['record']:
                            frame=command.get('frame',state['frame'])
                            if not isinstance(frame,(int,float)):raise ValueError('Invalid pose time.')
                            frame=max(state['record']['start'],min(state['frame'],int(frame)))
                            state['record']['events'].append({'type':'pose','frame':frame,'pose':pose})
                    elif op in ('note_on','note_off','panic','snapshot','add','delete','move','set','input'):
                        proc.stdin.write((json.dumps(command)+'\n').encode());await proc.stdin.drain()
                    else:raise ValueError('Unknown operation.')
                except (ValueError,TypeError,OverflowError,OSError) as e:await ws.send_json({'type':'error','message':str(e)})
        finally:
            task.cancel()
            try:
                with contextlib.suppress(asyncio.CancelledError):await task
            finally:
                try:
                    record=state['record']
                    if record and record['pcm']:
                        record['plugin']=state['plugin']
                        await asyncio.get_running_loop().run_in_executor(self.exports,export_film,session,record,ROOT)
                finally:
                    if proc.returncode is None:proc.terminate()
                    await proc.wait();self.busy=False
        return ws

def validate_pose(pose):
    import math
    if not isinstance(pose,dict) or not isinstance(pose.get('poses'),dict):raise ValueError('Invalid pose.')
    for key,limit in [('explode',1),('angle',70)]:
        value=pose.get(key)
        if not isinstance(value,(int,float)) or not math.isfinite(value) or abs(value)>limit:raise ValueError('Invalid camera value.')
    if pose.get('highlight') is not None and (not isinstance(pose['highlight'],str) or len(pose['highlight'])>256):raise ValueError('Invalid highlight.')
    if len(pose['poses'])>64:raise ValueError('Too many poses.')
    for part,p in pose['poses'].items():
        if not isinstance(part,str) or not isinstance(p,dict):raise ValueError('Invalid part.')
        for k,v in p.items():
            if k not in ('x','y','z','rotateX','rotateY','rotateZ','scaleX','scaleY') or not isinstance(v,(int,float)) or not math.isfinite(v) or abs(v)>2000:raise ValueError('Invalid transform.')
            if k.startswith('scale') and not .1<=v<=4:raise ValueError('Scale must be .1..4.')

@web.middleware
async def loopback(request,handler):
    if request.host.split(':')[0] not in ('localhost','127.0.0.1'):raise web.HTTPForbidden()
    response=await handler(request);response.headers['Cache-Control']='no-store';return response

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--sessions',type=Path,default=Path('/tmp/mui-motion-bridge-sessions'));p.add_argument('--port',type=int,default=3020);a=p.parse_args()
    if not a.binary.is_file() or not os.access(a.binary,os.X_OK):p.error('Build the plugin adapter executable first.')
    a.sessions.mkdir(parents=True,exist_ok=True);gateway=Gateway(a.binary.resolve(),a.sessions.resolve(),a.port)
    app=web.Application(middlewares=[loopback],client_max_size=65536)
    app.router.add_get('/ws',gateway.socket)
    files={'/':'index.html','/input.mjs':'input.mjs','/client.mjs':'client.mjs','/stream-worklet.mjs':'stream-worklet.mjs'}
    for route,file in files.items():app.router.add_get(route,lambda request,file=file:web.FileResponse(HERE/file))
    app.router.add_get('/layers.mjs',lambda r:web.FileResponse(ROOT/'tools/film/layers.mjs'))
    app.router.add_get('/Inter-V.otf',lambda r:web.FileResponse(inter(ROOT)/'ttf/Inter-V.otf'))
    app.router.add_static('/sessions/',a.sessions.resolve(),show_index=False)
    async def cleanup(app):gateway.exports.shutdown(wait=False,cancel_futures=True)
    app.on_cleanup.append(cleanup)
    print(f'MUI Motion: http://localhost:{a.port}',flush=True);web.run_app(app,host='127.0.0.1',port=a.port)
if __name__=='__main__':main()
