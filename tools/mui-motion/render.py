#!/usr/bin/env python3
"""Render a frozen MUI take and retain broadband audio detail in the MP4."""
import argparse,os,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('project',type=Path);p.add_argument('--output',type=Path);a=p.parse_args()
project=a.project.resolve();output=(a.output or project/'performance.mp4').resolve()
subprocess.run(['npx','--yes','hyperframes@0.8.66','render',str(project),'--quality','delivery','--fps','30','--output',str(output)],check=True)
# The CLI's default AAC setting removes substantial high-frequency noise energy.
# Keep its rendered video and encode the original captured PCM at higher bitrate.
with tempfile.TemporaryDirectory(prefix='mui-audio-',dir=output.parent) as temp:
    mux=Path(temp)/'delivery.mp4'
    subprocess.run(['ffmpeg','-v','error','-i',str(output),'-i',str(project/'assets/audio.wav'),'-map','0:v:0','-map','1:a:0','-c:v','copy','-c:a','aac','-b:a','512k','-cutoff','24000','-movflags','+faststart','-y',str(mux)],check=True)
    os.replace(mux,output)
print(output)
