"""Run a command and save elapsed time, exit code, logs and sampled process RSS."""
import argparse, json, os, subprocess, sys, time, ctypes
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import psutil
parser=argparse.ArgumentParser();parser.add_argument('--name',required=True);parser.add_argument('command',nargs=argparse.REMAINDER);args=parser.parse_args()
command=args.command
if command and command[0]=='--': command=command[1:]
out=Path('artifacts/performance');out.mkdir(parents=True,exist_ok=True)
started=time.perf_counter();samples=[];peak=0
with (out/f'{args.name}.log').open('w',encoding='utf-8') as log:
    process=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT)
    monitored=psutil.Process(process.pid)
    while process.poll() is None:
        try:
            processes=[monitored]+monitored.children(recursive=True)
            rss=sum(p.memory_info().rss for p in processes if p.is_running());peak=max(peak,rss)
            samples.append({'seconds':round(time.perf_counter()-started,3),'rssBytes':rss})
        except psutil.Error: pass
        time.sleep(.1)
result={'name':args.name,'command':command,'exitCode':process.returncode,'elapsedSeconds':round(time.perf_counter()-started,3),'peakSampledWorkingSetBytes':peak,'sampleIntervalMs':100,'samples':samples}
if os.name=='nt':
    class Counters(ctypes.Structure):
        _fields_=[('cb',ctypes.c_ulong),('faults',ctypes.c_ulong)]+[(name,ctypes.c_size_t) for name in ('peakWorkingSet','workingSet','peakPaged','paged','peakNonPaged','nonPaged','pagefile','peakPagefile','private')]
    counters=Counters();counters.cb=ctypes.sizeof(counters)
    if ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(int(process._handle)),ctypes.byref(counters),ctypes.sizeof(counters)):result['peakParentWorkingSetBytes']=counters.peakWorkingSet
(out/f'{args.name}.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps({k:v for k,v in result.items() if k!='samples'},ensure_ascii=False));sys.exit(process.returncode)
