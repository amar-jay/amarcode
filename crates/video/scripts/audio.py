"""Quiet original nature bed: stream, wind, and a few distant birds."""
from pathlib import Path
import wave
import numpy as np

SR = 48000
DURATION = 30
rng = np.random.default_rng(23)
n = SR * DURATION
t = np.arange(n) / SR

def place(signal,start,pan=0):
    offset=int(start*SR); n=min(len(signal),len(t)-offset)
    if offset<0 or n<=0:return
    mix[offset:offset+n,0]+=signal[:n]*np.sqrt((1-pan)/2)
    mix[offset:offset+n,1]+=signal[:n]*np.sqrt((1+pan)/2)

def note(freq,duration,gain=.03):
    x=np.arange(int(SR*duration))/SR
    env=(1-np.exp(-x/.008))*np.exp(-x/1.0)*np.minimum((duration-x)/.15,1)
    return gain*env*(np.sin(2*np.pi*freq*x)+.16*np.sin(2*np.pi*2*freq*x)*np.exp(-x*4))

# Warm major-ninth harmony; no impacts or slide-transition swooshes.
for channel,detune in enumerate([.999,1.001]):
    pad=sum(np.sin(2*np.pi*freq*detune*t+i*.6)/(i+2) for i,freq in enumerate([130.81,196,246.94,293.66,329.63]))
    env=np.minimum(t/2,1)*np.minimum((30-t)/2,1)
    mix[:,channel]+=pad*.035*env
for beat in range(48):
    at=.5+beat*.6
    if at>28:break
    freq=[523.25,659.25,783.99,587.33,659.25,987.77,783.99,659.25][beat%8]
    place(note(freq,2.0,.035),at,(-.3 if beat%2 else .3))
    if beat%2==0:
        x=np.arange(int(SR*.22))/SR
        place(np.sin(2*np.pi*(62*x+1.2*(1-np.exp(-x*30))))*np.exp(-x*26)*.07,at)
# Soft pointer clicks synchronized to recorded actions.
manifest=Path(__file__).resolve().parent.parent/'public/walkthrough/manifest.json'
for action in json.loads(manifest.read_text())['actions']:
    if action['action']=='focus prompt':continue
    x=np.arange(int(SR*.035))/SR
    place((rng.normal(0,1,len(x))*.5+np.sin(x*2*np.pi*1700)) * np.exp(-x*210)*.022,action['time'])
for start,text in [(2.86,'Build a command palette for Orbit.'),(8.74,'Add a subtle entrance.'),(14.74,'Respect reduced motion, too.')]:
    for i in range(len(text)):
        x=np.arange(int(SR*.018))/SR
        place(rng.normal(0,1,len(x))*np.exp(-x*300)*.005,start+.024*i,-.1)
place(note(523.25,3.7,.035),26.2)
place(note(659.25,3.7,.025),26.2,.2)
place(note(783.99,3.7,.02),26.2,-.2)
mix*=np.clip(t/.15,0,1)[:,None]*np.clip((30-t)/1.1,0,1)[:,None]
mix=np.tanh(mix*1.3)
mix*=.65/max(np.max(np.abs(mix)),.001)
output=Path(__file__).resolve().parent.parent/'public/audio/score.wav'
with wave.open(str(output),'wb') as f:
    f.setnchannels(2);f.setsampwidth(2);f.setframerate(SR);f.writeframes((mix*32767).astype('<i2').tobytes())
print(output)
