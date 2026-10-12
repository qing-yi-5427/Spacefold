"""Sample one owned Spacefold process and its WebView2 tree, without unrelated browser tabs."""
import sys,time,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import psutil
pid=int(sys.argv[1]);out=Path(sys.argv[2]);duration=float(sys.argv[3]);parent=psutil.Process(pid)
assert parent.name().lower()=='spacefold.exe'
started=time.perf_counter();samples=[];peak=0
while time.perf_counter()-started<duration:
    try:
        processes=[parent]+parent.children(recursive=True);rss=sum(p.memory_info().rss for p in processes if p.is_running());peak=max(peak,rss)
        samples.append({'elapsedMs':round((time.perf_counter()-started)*1000),'processTreeWorkingSetBytes':rss,'processCount':len(processes)})
    except psutil.Error:pass
    time.sleep(.01)
out.parent.mkdir(parents=True,exist_ok=True);result={'pid':pid,'sampleIntervalMs':10,'durationSeconds':duration,'peakSampledProcessTreeWorkingSetBytes':peak,'sampledPeakIsLowerBound':True,'includesTauriAndWebView2':True,'samples':samples};out.write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k!='samples'}))
