// A bounded stereo jitter buffer. The reported frame is the audio sample being
// played, not the packet's arrival time. The scope observes this same output.
class MuiStream extends AudioWorkletProcessor {
 constructor(options){super();this.bufferFrames=Math.max(2400,Math.min(12000,options.processorOptions?.bufferFrames||7200));this.queue=[];this.offset=0;this.queued=0;this.started=false;this.frame=0;this.underruns=0;this.dropped=0;this.ticks=0;
 this.port.onmessage=({data})=>{if(data.reset){this.queue=[];this.offset=0;this.queued=0;this.started=false;return;}
 const view=new DataView(data);const start=Number(view.getBigUint64(0,true));const pcm=new Float32Array(data,8);
 if(this.queued>48000*.3){this.queue=[];this.offset=0;this.queued=0;this.started=false;this.dropped++;}
 this.queue.push({start,pcm});this.queued+=pcm.length/2;
 };}
 process(inputs,outputs){const [l,r]=outputs[0];if(!r)return true;
 if(!this.started&&this.queued>=this.bufferFrames)this.started=true;
 for(let i=0;i<l.length;i++){
 const block=this.queue[0];if(!this.started||!block){l[i]=r[i]=0;if(this.started){this.started=false;this.underruns++;}continue;}
 l[i]=block.pcm[this.offset*2];r[i]=block.pcm[this.offset*2+1];this.frame=block.start+this.offset;this.offset++;this.queued--;
 if(this.offset===block.pcm.length/2){this.queue.shift();this.offset=0;}
 }
 if(++this.ticks%8===0)this.port.postMessage({frame:this.frame,queued:this.queued,underruns:this.underruns,dropped:this.dropped});return true;
 }
}
registerProcessor('mui-stream',MuiStream);
