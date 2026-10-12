"""Rotated same-machine benchmarks. All reports and DBs stay outside the sources."""
import ctypes,json,os,re,subprocess,sys,threading,time,sqlite3
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import psutil
ROOT=Path.cwd();OUT=ROOT/os.environ.get('BENCHMARK_OUTPUT','artifacts/benchmarks');OUT.mkdir(parents=True,exist_ok=True)
GDU=ROOT/'artifacts/bench-tools/gdu/gdu_windows_amd64.exe'
DUA=ROOT/'artifacts/bench-tools/dua/dua-v2.45.1-x86_64-pc-windows-msvc/dua.exe'
WDS=ROOT/'artifacts/bench-tools/windirstat/x64/WinDirStat.exe'
SF=ROOT/'src-tauri/target/release/spacefold-probe.exe'
class Counters(ctypes.Structure):
    _fields_=[('cb',ctypes.c_ulong),('faults',ctypes.c_ulong)]+[(name,ctypes.c_size_t) for name in ('peakWorkingSet','workingSet','peakPaged','paged','peakNonPaged','nonPaged','pagefile','peakPagefile','private')]
def run(name,command,report=None):
    start=time.perf_counter();first=None;first_report=None;output=bytearray();samples=[];peak=0
    startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
    process=subprocess.Popen([str(v) for v in command],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,startupinfo=startup)
    def read():
        nonlocal first
        while True:
            data=process.stdout.read1(65536)
            if not data:break
            if first is None:first=time.perf_counter()-start
            output.extend(data)
    reader=threading.Thread(target=read);reader.start();monitored=psutil.Process(process.pid)
    while process.poll() is None:
        if report is not None and first_report is None and report.exists() and report.stat().st_size>0:first_report=time.perf_counter()-start
        try:
            rss=sum(p.memory_info().rss for p in [monitored]+monitored.children(recursive=True) if p.is_running());peak=max(peak,rss)
            samples.append({'seconds':round(time.perf_counter()-start,4),'rssBytes':rss})
        except psutil.Error:pass
        time.sleep(.01)
    reader.join();elapsed=time.perf_counter()-start;counter=Counters();counter.cb=ctypes.sizeof(counter)
    if report is not None and first_report is None and report.exists() and report.stat().st_size>0:first_report=elapsed
    ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(int(process._handle)),ctypes.byref(counter),ctypes.sizeof(counter))
    text=output.decode('utf-8',errors='replace');(OUT/f'{name}.log').write_text(text,encoding='utf-8')
    result={'name':name,'command':[str(v) for v in command],'exitCode':process.returncode,'elapsedSeconds':round(elapsed,4),'firstOutputSeconds':None if first is None else round(first,4),'firstNonemptyReportSeconds':None if first_report is None else round(first_report,4),'peakParentWorkingSetBytes':counter.peakWorkingSet,'peakSampledProcessTreeWorkingSetBytes':peak,'sampleIntervalMs':10,'samples':samples}
    (OUT/f'{name}.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    assert process.returncode==0,(name,text[:1000])
    return result,text
def expected(source):
    files=dirs=0;size=0
    for base,children,names in os.walk(source,followlinks=False):
        children[:]=[name for name in children if not (Path(base)/name).is_symlink()]
        dirs+=len(children)
        for name in names:
            file=Path(base)/name
            if file.is_symlink():continue
            files+=1;size+=file.stat().st_size
    return {'files':files,'directories':dirs,'entriesExcludingRoot':files+dirs,'logicalBytes':str(size)}
def count_gdu(item):
    if isinstance(item,dict):return int('name' in item)+sum(count_gdu(value) for value in item.values())
    if isinstance(item,list):return sum(count_gdu(value) for value in item)
    return 0
sources={'small100k':ROOT/'artifacts/benchmark-source','real-msvc-headers':Path('C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include')}
tools=['gdu-summary','spacefold-kernel','dua','gdu-full-tree','windirstat-full-tree','spacefold-sqlite','gdu-sqlite']
all_results=[]
for dataset,source in sources.items():
    exp=expected(source);(OUT/f'{dataset}-expected.json').write_text(json.dumps({'source':str(source),**exp},indent=2),encoding='utf-8')
    for round_index in range(3):
        order=tools[round_index:]+tools[:round_index]
        for tool in order:
            name=f'{dataset}-round{round_index+1}-{tool}';report=OUT/f'{name}-report.json';data=OUT/f'{name}-db'
            if tool=='gdu-summary':command=[GDU,'-a','-n','-p','--no-prefix','--no-delete','--no-view-file','-s',source]
            elif tool=='gdu-full-tree':command=[GDU,'-a','-p','--no-delete','--no-view-file','-o',report,source]
            elif tool=='dua':command=[DUA,'--apparent-size','--count-hard-links','--format','bytes','--threads','0','aggregate','--stats',source]
            elif tool=='windirstat-full-tree':command=[WDS,'/SaveTo',report,source]
            elif tool=='gdu-sqlite':command=[GDU,'-a','-n','-p','--no-prefix','--no-delete','--no-view-file','-s',f'--db={OUT/(name+".sqlite")}',source]
            elif tool=='spacefold-kernel':command=[SF,'kernel',source]
            else:command=[SF,'scan',data,source]
            result,text=run(name,command,report if tool in ('gdu-full-tree','windirstat-full-tree') else None);result.update({'tool':tool,'dataset':dataset,'round':round_index+1,'expected':exp})
            if tool=='gdu-summary':assert int(re.search(r'^\s*(\d+)',text).group(1))==int(exp['logicalBytes'])
            elif tool=='gdu-full-tree':
                doc=json.loads(report.read_text(encoding='utf-8'));assert doc[3][0]['asize']==int(exp['logicalBytes']);assert count_gdu(doc)==exp['entriesExcludingRoot']+1
            elif tool=='dua':
                assert int(re.search(r'(\d+) b total',text).group(1))==int(exp['logicalBytes']);assert int(re.search(r'entries_traversed: (\d+)',text).group(1))==exp['entriesExcludingRoot']
            elif tool=='windirstat-full-tree':
                doc=json.loads(report.read_text(encoding='utf-8-sig'));assert len(doc)==exp['entriesExcludingRoot']+1;assert doc[0]['逻辑大小']==int(exp['logicalBytes']);assert doc[0]['文件数']==exp['files']
            elif tool=='gdu-sqlite':
                assert int(re.search(r'^\s*(\d+)',text).group(1))==int(exp['logicalBytes'])
                with sqlite3.connect(OUT/(name+'.sqlite')) as db:
                    assert db.execute('SELECT COUNT(*) FROM items').fetchone()[0]==exp['entriesExcludingRoot']+1
                    assert db.execute('SELECT size FROM items WHERE parent_id IS NULL').fetchone()[0]==int(exp['logicalBytes'])
                    assert db.execute('SELECT COUNT(*) FROM items WHERE is_dir=0').fetchone()[0]==exp['files']
                    assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
            else:
                doc=json.loads(text.strip().splitlines()[-1]);scan=doc.get('scan',doc);assert scan['logicalTotalBytes']==exp['logicalBytes'];assert str(scan.get('discoveredCount',scan.get('count')))==str(exp['entriesExcludingRoot']+(tool=='spacefold-sqlite'))
                if 'firstResultElapsedMs' in doc:result['firstResultElapsedMs']=doc['firstResultElapsedMs']
                if 'elapsedMs' in doc:result['internalElapsedMs']=doc['elapsedMs']
            result['correctnessVerified']=True;result.pop('samples',None);all_results.append(result)
            (OUT/'results.json').write_text(json.dumps({'cachePolicy':'Prepared fixture plus expected reference walk; natural warm caches, no cache flushing or security changes. Rotated order per round.','results':all_results},indent=2),encoding='utf-8')
            print(json.dumps({k:v for k,v in result.items() if k not in ('command','expected')},ensure_ascii=False),flush=True)

