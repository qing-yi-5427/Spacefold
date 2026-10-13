import { afterEach, describe, expect, it, vi } from 'vitest';
import type { JobEvent, NodePage, ViewSnapshot } from './contracts';
import { FixtureDataSource } from './data';
import { Workspace } from './workspace';

function deferred<T>() {
  let resolve!: (value:T)=>void, reject!: (reason:unknown)=>void;
  const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});
  return {promise,resolve,reject};
}
async function ready() {
  const source=new FixtureDataSource(),workspace=new Workspace();
  workspace.set({source});await workspace.adoptScan(source.fixtureScan);
  return {source,workspace,find:(name:string)=>[...source.data.nodes.values()].find(node=>node.name===name)!};
}
function request(workspace:Workspace) {
  const s=workspace.state;
  return {scanId:s.scan!.scanId,scopeId:s.scopeId!,expandedIds:[...s.expanded],selectedIds:[...s.selectedIds],primaryId:s.primaryId,sort:s.sort};
}
function event(seq:number,revision:number):JobEvent {
  return {jobId:'fixture',scanId:'fixture',type:'scan',seq,revision,status:'running',discoveredCount:String(seq),logicalTotalBytes:String(seq*100),errorCount:'0',writtenCount:'0',elapsedMs:seq*100,phase:'enumerating'};
}
afterEach(()=>{vi.clearAllTimers();vi.useRealTimers();});

describe('committed view refreshes remain visible and bound to current intent',()=>{
  it('publishes a delayed snapshot atomically while a sustained progress stream advances',async()=>{
    vi.useFakeTimers();
    const {source,workspace}=await ready(),before=workspace.state,view=workspace.getViewSnapshot();
    source.fixtureScan={...source.fixtureScan,revision:2,status:'running'};
    const snapshot=await source.viewSnapshot(request(workspace));
    const pending=deferred<ViewSnapshot>();vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData(),changes:typeof workspace.state[]=[];
    workspace.subscribe(()=>changes.push(workspace.state));
    for(let seq=1;seq<=30;seq++)workspace.onEvent(event(seq,seq+2));
    expect(workspace.state.scan?.discoveredCount).toBe('30');
    expect(workspace.state.scan?.revision).toBe(1);
    expect(workspace.getViewSnapshot()).toBe(view);
    expect(workspace.state.pages).toBe(before.pages);
    expect(workspace.state.chart).toBe(before.chart);
    expect(workspace.state.summary).toBe(before.summary);
    pending.resolve(snapshot);await refreshing;
    expect(workspace.state.scan?.revision).toBe(2);
    expect(workspace.state.scan?.discoveredCount).toBe('30');
    expect(workspace.state.chart?.revision).toBe(2);
    expect(workspace.state.summary).toEqual(snapshot.summary);
    expect(workspace.state.selectedIds).toEqual(before.selectedIds);
    expect(changes.every(state=>state.pages.size>0&&state.chart&&state.summary)).toBe(true);
    expect(workspace.getViewSnapshot()).not.toBe(view);
  });

  it('does not put a previous scope response under a newly entered directory',async()=>{
    vi.useFakeTimers();const {source,workspace,find}=await ready();
    const old=await source.viewSnapshot(request(workspace)),pending=deferred<ViewSnapshot>();
    vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();await workspace.enter(find('Games').nodeId);
    const current=workspace.state;
    pending.resolve(old);await refreshing;
    expect(workspace.state.scopeId).toBe(find('Games').nodeId);
    expect(workspace.state.pages).toBe(current.pages);
    expect(workspace.state.chart).toBe(current.chart);
    expect(workspace.state.selectedIds).toEqual(current.selectedIds);
  });

  it('discards a selection response after a newer selection and its summary resolve',async()=>{
    vi.useFakeTimers();const {source,workspace,find}=await ready();
    const old=await source.viewSnapshot(request(workspace)),pending=deferred<ViewSnapshot>();
    vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();await workspace.select(find('Games').nodeId);
    const summary=workspace.state.summary;
    pending.resolve(old);await refreshing;
    expect(workspace.state.primaryId).toBe(find('Games').nodeId);
    expect(workspace.state.summary).toBe(summary);
  });

  it('does not replace a changed primary item when the selected IDs remain identical',async()=>{
    vi.useFakeTimers();const {source,workspace,find}=await ready(),ai=find('AI Models'),games=find('Games');
    workspace.set({selectedIds:[ai.nodeId,games.nodeId],primaryId:ai.nodeId});await workspace.updateSummary();
    const old=await source.viewSnapshot(request(workspace)),pending=deferred<ViewSnapshot>();
    vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();workspace.set({primaryId:games.nodeId});await workspace.updateSummary();
    pending.resolve(old);await refreshing;
    expect(workspace.state.selectedIds).toEqual([ai.nodeId,games.nodeId]);
    expect(workspace.state.primaryId).toBe(games.nodeId);
    expect(workspace.state.summary?.normalizedIds).toEqual([ai.nodeId,games.nodeId]);
  });

  it('never revives content from a scan replaced while a refresh was in flight',async()=>{
    vi.useFakeTimers();const {source,workspace}=await ready();
    const old=await source.viewSnapshot(request(workspace)),pending=deferred<ViewSnapshot>();
    vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();
    source.fixtureScan={...source.fixtureScan,scanId:'second'};
    await workspace.adoptScan(source.fixtureScan);
    const current=workspace.state;
    pending.resolve(old);await refreshing;
    expect(workspace.state.scan?.scanId).toBe('second');
    expect(workspace.state.pages).toBe(current.pages);
    expect(workspace.state.chart).toBe(current.chart);
  });

  it('ignores a late error for a scope the user already left',async()=>{
    vi.useFakeTimers();const {source,workspace,find}=await ready();
    const pending=deferred<ViewSnapshot>();vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();await workspace.enter(find('Games').nodeId);
    pending.reject({code:'NODE_NOT_FOUND',message:'old scope'});await refreshing;
    expect(workspace.state.error).toBeNull();
    expect(workspace.state.scopeId).toBe(find('Games').nodeId);
  });

  it('does not undo a newer pagination intent while its snapshot is in flight',async()=>{
    vi.useFakeTimers();const {source,workspace}=await ready();
    const old=await source.viewSnapshot(request(workspace)),pending=deferred<ViewSnapshot>();
    vi.spyOn(source,'viewSnapshot').mockReturnValue(pending.promise);
    const refreshing=workspace.refreshData();
    const pages=new Map(workspace.state.pages),root=pages.get('0')!;
    pages.set('0',{...root,cursor:'later',ids:root.ids.slice(2)});workspace.set({pages});
    pending.resolve(old);await refreshing;
    expect(workspace.state.pages).toBe(pages);
    expect(workspace.state.pages.get('0')?.cursor).toBe('later');
  });

  it('preserves a collapsed selected ancestor and a collapsed scan root across background refreshes',async()=>{
    const {workspace,find}=await ready();
    workspace.set({expanded:new Set(['0']),selectedIds:[find('Checkpoints').nodeId],primaryId:find('Checkpoints').nodeId});
    await workspace.refreshData();
    expect([...workspace.state.expanded]).toEqual(['0']);
    expect(workspace.state.selectedIds).toEqual([find('Checkpoints').nodeId]);
    workspace.set({expanded:new Set()});await workspace.refreshData();
    expect(workspace.state.expanded.size).toBe(0);
    expect(workspace.state.pages.has('0')).toBe(true);
    expect(workspace.state.nodes.has(find('AI Models').nodeId)).toBe(true);
  });

  it('immediately removes the previous operation summary while entering a graphical scope',async()=>{
    const {source,workspace,find}=await ready();await workspace.switchView('map');
    const games=find('Games'),pages=new Map(workspace.state.pages);pages.delete(games.nodeId);workspace.set({pages});
    const page=await source.children({scanId:'fixture',parentId:games.nodeId,revision:1});
    const children=source.children.bind(source),pending=deferred<NodePage>();
    vi.spyOn(source,'children').mockImplementation(request=>request.parentId===games.nodeId?pending.promise:children(request));
    const entering=workspace.enter(games.nodeId);
    expect(workspace.state.scopeId).toBe(games.nodeId);
    expect(workspace.state.selectedIds).toEqual([]);
    expect(workspace.state.primaryId).toBeNull();
    expect(workspace.state.summary).toBeNull();
    pending.resolve(page);await entering;
    expect(workspace.state.primaryId).toBe(find('SteamLibrary').nodeId);
    expect(workspace.state.summary?.nodes[0].name).toBe('SteamLibrary');
  });

  it('does not restore the first scope or its selection after two delayed directory navigations',async()=>{
    const {source,workspace,find}=await ready(),games=find('Games'),media=find('Media');
    const pages=new Map(workspace.state.pages);pages.delete(games.nodeId);workspace.set({pages});
    const page=await source.children({scanId:'fixture',parentId:games.nodeId,revision:1});
    const children=source.children.bind(source),pending=deferred<NodePage>();
    vi.spyOn(source,'children').mockImplementation(request=>request.parentId===games.nodeId?pending.promise:children(request));
    const older=workspace.enter(games.nodeId);
    expect(workspace.state.selectedIds).toEqual([games.nodeId]);
    expect(workspace.state.summary).toBeNull();
    await workspace.enter(media.nodeId);
    const summary=workspace.state.summary;
    pending.resolve(page);expect(await older).toBe(false);
    expect(workspace.state.scopeId).toBe(media.nodeId);
    expect(workspace.state.selectedIds).toEqual([media.nodeId]);
    expect(workspace.state.summary).toBe(summary);
    expect(workspace.state.chart?.root.node.nodeId).toBe(media.nodeId);
  });

  it('does not overwrite a user selection made while parent navigation is waiting',async()=>{
    const {source,workspace,find}=await ready(),ai=find('AI Models'),checkpoints=find('Checkpoints');
    await workspace.enter(ai.nodeId);
    const pages=new Map(workspace.state.pages);pages.delete('0');workspace.set({pages});
    const page=await source.children({scanId:'fixture',parentId:'0',revision:1});
    const children=source.children.bind(source),pending=deferred<NodePage>();
    vi.spyOn(source,'children').mockImplementation(request=>request.parentId==='0'?pending.promise:children(request));
    const navigating=workspace.navigate('0'),selecting=workspace.select(checkpoints.nodeId);
    pending.resolve(page);await Promise.all([navigating,selecting]);
    expect(workspace.state.scopeId).toBe('0');
    expect(workspace.state.primaryId).toBe(checkpoints.nodeId);
    expect(workspace.state.summary?.nodes[0].name).toBe('Checkpoints');
  });

  it('retains the complete view and reports a real backend failure',async()=>{
    const {source,workspace}=await ready(),before=workspace.state;
    vi.spyOn(source,'viewSnapshot').mockRejectedValue({code:'VIEW_PAYLOAD_BUDGET',message:'视图返回量超限'});
    await workspace.refreshData();
    expect(workspace.state.error?.code).toBe('VIEW_PAYLOAD_BUDGET');
    expect(workspace.state.pages).toBe(before.pages);
    expect(workspace.state.chart).toBe(before.chart);
    expect(workspace.state.summary).toBe(before.summary);
  });

  it('rejects a mixed revision response without committing any of its content',async()=>{
    const {source,workspace}=await ready(),before=workspace.state;
    const snapshot=await source.viewSnapshot(request(workspace));snapshot.chart.revision++;
    vi.spyOn(source,'viewSnapshot').mockResolvedValue(snapshot);
    await workspace.refreshData();
    expect(workspace.state.error?.code).toBe('VIEW_REVISION_MISMATCH');
    expect(workspace.state.pages).toBe(before.pages);
    expect(workspace.state.chart).toBe(before.chart);
  });

  it('keeps a directory page anchored by node identity instead of replaying an old revision cursor',async()=>{
    const {source,workspace,find}=await ready(),ai=find('AI Models'),llm=find('LLM');
    const snapshot=await source.viewSnapshot({...request(workspace),pageAnchors:[{parentId:ai.nodeId,firstId:llm.nodeId}]});
    expect(snapshot.pages.find(value=>value.parentId===ai.nodeId)?.page.nodes[0].nodeId).toBe(llm.nodeId);
    expect(snapshot.pages.find(value=>value.parentId===ai.nodeId)?.page.pageCursor).toBeTruthy();
    const fallback=await source.viewSnapshot({...request(workspace),pageAnchors:[{parentId:ai.nodeId,firstId:'missing'}]});
    expect(fallback.pages.find(value=>value.parentId===ai.nodeId)?.page.nodes[0].name).toBe('Checkpoints');
  });
});
