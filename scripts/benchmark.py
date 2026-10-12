"""Rotated same-machine benchmarks. All reports and DBs stay outside the sources."""
import argparse,ctypes,hashlib,json,os,re,shutil,statistics,subprocess,sys,threading,time,sqlite3
from pathlib import Path
from contextlib import closing
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import psutil
ROOT=Path.cwd();OUT=ROOT/os.environ.get('BENCHMARK_OUTPUT','artifacts/benchmarks');OUT.mkdir(parents=True,exist_ok=True)
GDU=ROOT/'artifacts/bench-tools/gdu/gdu_windows_amd64.exe'
DUA=ROOT/'artifacts/bench-tools/dua/dua-v2.45.1-x86_64-pc-windows-msvc/dua.exe'
WDS=ROOT/'artifacts/bench-tools/windirstat/x64/WinDirStat.exe'
SF=Path(os.environ.get('BENCHMARK_PROBE',ROOT/'src-tauri/target/release/spacefold-probe.exe')).resolve()
class Counters(ctypes.Structure):
    _fields_=[('cb',ctypes.c_ulong),('faults',ctypes.c_ulong)]+[(name,ctypes.c_size_t) for name in ('peakWorkingSet','workingSet','peakPaged','paged','peakNonPaged','nonPaged','pagefile','peakPagefile','private')]
class IoCounters(ctypes.Structure):
    _fields_=[(name,ctypes.c_ulonglong) for name in ('readOperations','writeOperations','otherOperations','readBytes','writeBytes','otherBytes')]
def run(name,command,report=None,database=None):
    start=time.perf_counter();first=None;first_report=None;output=bytearray();samples=[];peak=0;wal_peak=0;wal_observed=False
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
        if database is not None:
            wal=Path(str(database)+'-wal')
            try:
                wal_peak=max(wal_peak,wal.stat().st_size);wal_observed=True
            except FileNotFoundError:pass
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
    handle=ctypes.c_void_p(int(process._handle));created=ctypes.c_ulonglong();exited=ctypes.c_ulonglong();kernel=ctypes.c_ulonglong();user=ctypes.c_ulonglong()
    if ctypes.windll.kernel32.GetProcessTimes(handle,ctypes.byref(created),ctypes.byref(exited),ctypes.byref(kernel),ctypes.byref(user)):
        result.update({'parentKernelCpuSeconds':kernel.value/10000000,'parentUserCpuSeconds':user.value/10000000,'parentTotalCpuSeconds':(kernel.value+user.value)/10000000})
    io=IoCounters()
    if ctypes.windll.kernel32.GetProcessIoCounters(handle,ctypes.byref(io)):
        result['parentOsReportedIo']={field:getattr(io,field) for field,_ in IoCounters._fields_}
        result['ioBoundary']='OS parent-process counters, not physical device bytes; all children remain in sampled process-tree memory.'
    if database is not None:result.update({'peakSampledWalBytes':wal_peak if wal_observed else None,'walSampledPeakIsLowerBound':True,'walBytesAfterExit':Path(str(database)+'-wal').stat().st_size if Path(str(database)+'-wal').exists() else 0})
    (OUT/f'{name}.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    assert process.returncode==0,(name,text[:1000])
    return result,text
def expected(source):
    files=dirs=0;size=0;manifest={os.path.normcase(os.path.abspath(source)):('directory',0)}
    for base,children,names in os.walk(source,followlinks=False):
        children[:]=[name for name in children if not (Path(base)/name).is_symlink()]
        dirs+=len(children)
        for name in children:manifest[os.path.normcase(os.path.abspath(Path(base)/name))]=('directory',0)
        for name in names:
            file=Path(base)/name
            if file.is_symlink():continue
            length=file.stat().st_size;files+=1;size+=length
            manifest[os.path.normcase(os.path.abspath(file))]=('file',length)
    return {'files':files,'directories':dirs,'entriesExcludingRoot':files+dirs,'logicalBytes':str(size)},manifest
def verify_spacefold(data,scan,exp,manifest,index_layout='legacy'):
    """Independent read after process exit, outside timing. All original nodes retained."""
    database=data/'spacefold.sqlite3'
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)) as db:
        assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
        assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
        assert db.execute('PRAGMA journal_mode').fetchone()[0]=='wal'
        stored=db.execute('SELECT status,discovered,error_count,finished FROM scans WHERE id=?',(scan['scanId'],)).fetchone()
        assert stored[0]=='completed' and stored[1]==exp['entriesExcludingRoot']+1 and stored[2]==0 and stored[3]
        assert scan['status']=='completed' and scan['errorCount']=='0' and not scan['incomplete']
        assert db.execute('SELECT COUNT(*) FROM pending_dirs').fetchone()[0]==0
        indexes={row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='index'")}
        required={'nodes_parent_name_v2','nodes_parent_size_v3','nodes_incomplete','nodes_special_kind_v2'}
        if index_layout=='legacy':required.add('nodes_parent_size_asc_v2')
        else:assert 'nodes_parent_size_asc_v2' not in indexes,'Duplicate size index was not migrated away'
        assert required<=indexes,(required-indexes)
        rows=db.execute('SELECT id,parent_id,path,kind,payload,logical_total,file_count,dir_count,children_count,incomplete,error_code FROM nodes WHERE scan_id=?',(scan['scanId'],)).fetchall()
        assert len(rows)==exp['entriesExcludingRoot']+1
        actual={};nodes={};children={}
        for row in rows:
            ident,parent,raw,kind,payload,total,fc,dc,cc,incomplete,error=row
            display=raw.decode('utf-16-le');display=display.removeprefix('\\\\?\\')
            key=os.path.normcase(os.path.abspath(display));assert key not in actual
            own=json.loads(payload).get('b',0) if kind=='file' else 0
            actual[key]=(kind,own)
            assert not incomplete and error is None,(ident,incomplete,error)
            nodes[ident]={'parent':parent,'kind':kind,'own':own,'total':total,'fc':fc,'dc':dc,'cc':cc,'path':key}
            children.setdefault(parent,[]).append(ident)
        assert actual==manifest,'Full path/kind/original-byte manifest differs'
        assert children[None]==[1]
        for ident,node in nodes.items():
            members=[nodes[child] for child in children.get(ident,[])]
            if node['parent'] is not None:
                assert node['parent'] in nodes
                assert os.path.dirname(node['path'])==nodes[node['parent']]['path']
            assert node['cc']==len(members)
            if node['kind']=='directory':
                assert node['total']==sum(child['total'] for child in members)
                assert node['fc']==sum(child['fc'] for child in members)
                assert node['dc']==sum(child['dc']+(child['kind']=='directory') for child in members)
            else:
                assert node['total']==node['own'] and node['fc']==1 and node['dc']==0
        assert nodes[1]['total']==int(exp['logicalBytes']) and nodes[1]['fc']==exp['files'] and nodes[1]['dc']==exp['directories']
        plan=db.execute('EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE scan_id=? AND parent_id=1 ORDER BY sort_size,name COLLATE NOCASE,id LIMIT 200',(scan['scanId'],)).fetchall()
        assert any('nodes_parent_size_v3' in row[3] for row in plan),plan
        assert not any('TEMP B-TREE' in row[3] for row in plan),plan
        reverse_plan=[]
        if index_layout!='legacy':
            reverse_plan=db.execute('EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE scan_id=? AND parent_id=1 ORDER BY sort_size DESC,name COLLATE NOCASE DESC,id DESC LIMIT 200',(scan['scanId'],)).fetchall()
            assert any('nodes_parent_size_v3' in row[3] for row in reverse_plan),reverse_plan
            assert not any('TEMP B-TREE' in row[3] for row in reverse_plan),reverse_plan
        return {'independentDatabaseRead':True,'fullPathAndRawBytesVerified':True,'allDirectoryAggregatesVerified':True,'completeNodes':len(rows),'indexes':sorted(required),'indexLayout':index_layout,'sizePageQueryPlan':plan,'sizeAscReverseQueryPlan':reverse_plan,'journalMode':'wal','pageSize':db.execute('PRAGMA page_size').fetchone()[0],'databaseBytes':database.stat().st_size}
def history_snapshot(database):
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)) as db:
        scan_columns=[row[1] for row in db.execute("PRAGMA table_info('scans')")]; node_columns=[column[0] for column in db.execute('SELECT * FROM nodes LIMIT 0').description]
        scans=[[{'blobHex':v.hex()} if isinstance(v,bytes) else v for v in row] for row in db.execute('SELECT '+','.join(scan_columns)+' FROM scans ORDER BY id')]
        digest=hashlib.sha256();count=0
        for row in db.execute('SELECT * FROM nodes ORDER BY scan_id,id'):
            values=[value.hex() if isinstance(value,bytes) else value for value in row]
            digest.update(json.dumps(values,separators=(',',':')).encode('utf-8'));count+=1
        return {'scanRecords':scans,'scanColumns':scan_columns,'nodeColumns':node_columns,'allStoredColumnsVerified':True,'historyNodeCount':count,'historyNodeDigest':digest.hexdigest(),'pageSize':db.execute('PRAGMA page_size').fetchone()[0]}
def verify_history(database,seed):
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)) as db:
        digest=hashlib.sha256();count=0
        for scan in seed['scanRecords']:
            actual=db.execute('SELECT '+','.join(seed['scanColumns'])+' FROM scans WHERE id=?',(scan[0],)).fetchone()
            actual=[{'blobHex':v.hex()} if isinstance(v,bytes) else v for v in actual]
            assert actual==scan,'Historical scan metadata changed'
            for row in db.execute('SELECT * FROM nodes WHERE scan_id=? ORDER BY id',(scan[0],)):
                values=[value.hex() if isinstance(value,bytes) else value for value in row]
                digest.update(json.dumps(values,separators=(',',':')).encode('utf-8'));count+=1
        assert count==seed['historyNodeCount'] and digest.hexdigest()==seed['historyNodeDigest'],'Historical tree changed'
        assert db.execute('PRAGMA page_size').fetchone()[0]==seed['pageSize']
        return {'historicalTreeUnchanged':True,**seed}
def count_gdu(item):
    if isinstance(item,dict):return int('name' in item)+sum(count_gdu(value) for value in item.values())
    if isinstance(item,list):return sum(count_gdu(value) for value in item)
    return 0
parser=argparse.ArgumentParser();parser.add_argument('--rounds',type=int,default=5);parser.add_argument('--sources',type=Path);parser.add_argument('--tools',default='gdu-summary,spacefold-kernel,dua,gdu-full-tree,windirstat-full-tree,spacefold-sqlite,gdu-sqlite');parser.add_argument('--early-gate-ratio',type=float,default=1.05);parser.add_argument('--seed-database',type=Path);parser.add_argument('--index-layout',choices=('current','legacy'),default='current');parser.add_argument('--comparison-probe',type=Path);args=parser.parse_args()
assert args.rounds>=5,'Formal comparisons require at least five rounds'
assert not os.environ.get('SPACEFOLD_PROFILE'),'Formal benchmark must use the normal unprofiled release path'
sources={'small100k':ROOT/'artifacts/benchmark-source','real-msvc-headers':Path('C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include')}
if args.sources:sources={key:Path(value).resolve() for key,value in json.loads(args.sources.read_text(encoding='utf-8-sig')).items()}
tools=args.tools.split(',')
allowed_tools={'gdu-summary','spacefold-kernel','dua','gdu-full-tree','windirstat-full-tree','spacefold-sqlite','gdu-sqlite','spacefold-old-sqlite'}
assert set(tools)<=allowed_tools, 'Unknown benchmark tool'
if args.comparison_probe:args.comparison_probe=args.comparison_probe.resolve()
assert ('spacefold-old-sqlite' not in tools or (args.comparison_probe and not args.seed_database)), 'Old comparison needs an explicit immutable probe and fresh databases'
executables=(SF,GDU,DUA,WDS)+((args.comparison_probe,) if args.comparison_probe else ())
seed=None
seed_destinations=set()
if args.seed_database:
    args.seed_database=args.seed_database.resolve();seed=history_snapshot(args.seed_database)
    assert seed['pageSize']==4096 and seed['historyNodeCount']>0,'Legacy seed must contain real 4KiB history'
    assert not Path(str(args.seed_database)+'-wal').exists() or Path(str(args.seed_database)+'-wal').stat().st_size==0,'Seed must already be checkpointed'
    (OUT/'legacy-seed.json').write_text(json.dumps({'path':str(args.seed_database),'sha256':hashlib.sha256(args.seed_database.read_bytes()).hexdigest(),**seed},indent=2),encoding='utf-8')
    for dataset in sources:
        for round_index in range(args.rounds):
            destination=OUT/f'{dataset}-round{round_index+1}-spacefold-sqlite-db'
            assert not destination.exists(),'Never reuse a seeded benchmark destination'
            destination.mkdir();shutil.copyfile(args.seed_database,destination/'spacefold.sqlite3');seed_destinations.add(destination)
assert not ctypes.windll.shell32.IsUserAnAdmin(),'Formal benchmark must run without elevation'
assert not any(p.info['name'].lower() in ('cargo.exe','rustc.exe','cl.exe','link.exe') for p in psutil.process_iter(['name']) if p.info['name']),'Concurrent compiler detected'
fingerprints={str(exe):hashlib.sha256(exe.read_bytes()).hexdigest() for exe in executables}
(OUT/'tool-fingerprints.json').write_text(json.dumps(fingerprints,indent=2),encoding='utf-8')
all_results=[]
for dataset,source in sources.items():
    exp,manifest=expected(source);(OUT/f'{dataset}-expected.json').write_text(json.dumps({'source':str(source),**exp},indent=2),encoding='utf-8')
    for round_index in range(args.rounds):
        pivot=(2*round_index)%len(tools);order=tools[pivot:]+tools[:pivot]
        if round_index%2:order.reverse()
        for tool in order:
            name=f'{dataset}-round{round_index+1}-{tool}';report=OUT/f'{name}-report.json';data=OUT/f'{name}-db'
            assert not report.exists() and (not data.exists() or data in seed_destinations) and not (OUT/(name+'.sqlite')).exists(),'Never reuse a benchmark destination'
            assert not any(p.info['name'].lower() in ('cargo.exe','rustc.exe','cl.exe','link.exe') for p in psutil.process_iter(['name']) if p.info['name']),'Concurrent compiler detected'
            if tool=='gdu-summary':command=[GDU,'-a','-n','-p','--no-prefix','--no-delete','--no-view-file','-s',source]
            elif tool=='gdu-full-tree':command=[GDU,'-a','-p','--no-delete','--no-view-file','-o',report,source]
            elif tool=='dua':command=[DUA,'--apparent-size','--count-hard-links','--format','bytes','--threads','0','aggregate','--stats',source]
            elif tool=='windirstat-full-tree':command=[WDS,'/SaveTo',report,source]
            elif tool=='gdu-sqlite':command=[GDU,'-a','-n','-p','--no-prefix','--no-delete','--no-view-file','-s',f'--db={OUT/(name+".sqlite")}',source]
            elif tool=='spacefold-kernel':command=[SF,'kernel',source]
            elif tool=='spacefold-old-sqlite':command=[args.comparison_probe,'scan',data,source]
            else:command=[SF,'scan',data,source]
            result,text=run(name,command,report if tool in ('gdu-full-tree','windirstat-full-tree') else None,data/'spacefold.sqlite3' if tool in ('spacefold-sqlite','spacefold-old-sqlite') else None);result.update({'tool':tool,'dataset':dataset,'round':round_index+1,'expected':exp})
            if tool=='gdu-summary':assert int(re.search(r'^\s*(\d+)',text).group(1))==int(exp['logicalBytes'])
            elif tool=='gdu-full-tree':
                doc=json.loads(report.read_text(encoding='utf-8'));assert doc[3][0]['asize']==int(exp['logicalBytes']);assert count_gdu(doc)==exp['entriesExcludingRoot']+1
            elif tool=='dua':
                assert int(re.search(r'(\d+) b total',text).group(1))==int(exp['logicalBytes']);assert int(re.search(r'entries_traversed: (\d+)',text).group(1))==exp['entriesExcludingRoot']
            elif tool=='windirstat-full-tree':
                doc=json.loads(report.read_text(encoding='utf-8-sig'));assert len(doc)==exp['entriesExcludingRoot']+1;assert doc[0]['逻辑大小']==int(exp['logicalBytes']);assert doc[0]['文件数']==exp['files']
            elif tool=='gdu-sqlite':
                assert int(re.search(r'^\s*(\d+)',text).group(1))==int(exp['logicalBytes'])
                with closing(sqlite3.connect(OUT/(name+'.sqlite'))) as db:
                    assert db.execute('SELECT COUNT(*) FROM items').fetchone()[0]==exp['entriesExcludingRoot']+1
                    assert db.execute('SELECT size FROM items WHERE parent_id IS NULL').fetchone()[0]==int(exp['logicalBytes'])
                    assert db.execute('SELECT COUNT(*) FROM items WHERE is_dir=0').fetchone()[0]==exp['files']
                    assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
            else:
                doc=json.loads(text.strip().splitlines()[-1]);scan=doc.get('scan',doc);assert scan['logicalTotalBytes']==exp['logicalBytes'];assert str(scan.get('discoveredCount',scan.get('count')))==str(exp['entriesExcludingRoot']+(tool in ('spacefold-sqlite','spacefold-old-sqlite')))
                result['probeSummary']=doc
                if tool in ('spacefold-sqlite','spacefold-old-sqlite'):result['persistenceVerification']=verify_spacefold(data,scan,exp,manifest,'legacy' if tool=='spacefold-old-sqlite' else args.index_layout)
                if tool=='spacefold-sqlite' and seed is not None:result['historyVerification']=verify_history(data/'spacefold.sqlite3',seed)
                if 'firstResultElapsedMs' in doc:result['firstResultElapsedMs']=doc['firstResultElapsedMs']
                if 'elapsedMs' in doc:result['internalElapsedMs']=doc['elapsedMs']
            result['correctnessVerified']=True;result.pop('samples',None);all_results.append(result)
            (OUT/'results.json').write_text(json.dumps({'cachePolicy':'Prepared fixture plus expected reference walk; natural warm caches, no cache flushing or security changes. Rotated order per round.','ordinaryUserToken':True,'rounds':args.rounds,'externalPollingIntervalMs':10,'probeTerminalPollingIntervalMs':50,'timingBoundary':'Process launch through exit and output reader completion; independent correctness read excluded, persistence and indexed terminal state included.','fingerprints':fingerprints,'results':all_results},indent=2),encoding='utf-8')
            print(json.dumps({k:v for k,v in result.items() if k not in ('command','expected')},ensure_ascii=False),flush=True)
        if dataset=='small100k' and round_index==1 and {'windirstat-full-tree','spacefold-sqlite'}<=set(tools):
            early={tool:statistics.median(r['elapsedSeconds'] for r in all_results if r['dataset']==dataset and r['tool']==tool) for tool in ('windirstat-full-tree','spacefold-sqlite')}
            ratio=early['spacefold-sqlite']/early['windirstat-full-tree']
            if ratio>args.early_gate_ratio:
                failure={'status':'early_gate_failed','completedRounds':2,'ratio':ratio,'mediansSeconds':early,'note':'Incomplete comparison, no final performance claim. Stop for further implementation work and restart in a fresh output directory.'}
                (OUT/'early-gate-failed.json').write_text(json.dumps(failure,indent=2),encoding='utf-8')
                print(json.dumps(failure),flush=True);sys.exit(3)
    final_exp,final_manifest=expected(source)
    assert final_exp==exp and final_manifest==manifest,'Dataset changed during benchmark'
summary={}
for dataset in sources:
    summary[dataset]={tool:{'medianSeconds':statistics.median(r['elapsedSeconds'] for r in all_results if r['dataset']==dataset and r['tool']==tool),'roundsSeconds':[r['elapsedSeconds'] for r in all_results if r['dataset']==dataset and r['tool']==tool]} for tool in tools}
(OUT/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
assert fingerprints=={str(exe):hashlib.sha256(exe.read_bytes()).hexdigest() for exe in executables},'Executable changed during benchmark'

