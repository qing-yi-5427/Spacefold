"""Read-only CPU background sample, outside every benchmark timing interval."""
import argparse,json,os,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import psutil

parser=argparse.ArgumentParser();parser.add_argument('output',type=Path);args=parser.parse_args()
started=time.perf_counter();before={};games=[]
for process in psutil.process_iter(['name']):
    if process.pid in (0,os.getpid()):continue
    try:
        before[process.pid]=(process.info['name'],sum(process.cpu_times()[:2]),time.perf_counter())
    except psutil.Error:pass
cpu=psutil.cpu_percent(interval=1,percpu=True);changes=[]
for process in psutil.process_iter(['name']):
    if process.info['name'] and process.info['name'].lower()=='yuanshen.exe':games.append({'pid':process.pid,'name':process.info['name']})
    if process.pid not in before:continue
    try:
        old=before[process.pid];delta=sum(process.cpu_times()[:2])-old[1];seconds=time.perf_counter()-old[2]
        if delta>0.01:changes.append({'pid':process.pid,'name':process.info['name'],'cpuSecondsDelta':round(delta,4),'processSampleWallSeconds':round(seconds,4),'percentOfOneLogicalCore':round(delta/seconds*100,2)})
    except psutil.Error:pass
result={'systemCpuSampleIntervalSeconds':1,'logicalCpuPercent':cpu,'meanCpuPercent':sum(cpu)/len(cpu),'processSnapshotWallSeconds':time.perf_counter()-started,'gameProcesses':games,'backgroundCpuProcesses':sorted(changes,key=lambda item:item['cpuSecondsDelta'],reverse=True),'note':'Read-only sample. Idle process and this helper excluded from process deltas. No priorities, caches, security settings or user processes changed.'}
args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps({'meanCpuPercent':result['meanCpuPercent'],'gameProcesses':games,'topCpuProcesses':result['backgroundCpuProcesses'][:5]}))
