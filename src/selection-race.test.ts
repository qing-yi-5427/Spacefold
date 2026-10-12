import { describe, expect, it, vi } from 'vitest';
import { FixtureDataSource } from './data';
import { Workspace } from './workspace';
import { OperationPreparation } from './operation-preparation';
import type { OperationPlan, OperationRequest, SelectionSummary } from './contracts';

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return {promise, resolve, reject};
}

async function ready() {
  const source = new FixtureDataSource(), workspace = new Workspace();
  workspace.set({source});
  await workspace.adoptScan(source.fixtureScan);
  return {source, workspace, find: (name: string) => [...source.data.nodes.values()].find(node => node.name === name)!};
}

describe('selection and operation requests remain bound to their intent', () => {
  it('immediately removes the old summary and ignores out of order selections and delayed action clicks', async () => {
    const {source, workspace, find} = await ready();
    const games = find('Games'), checkpoints = find('Checkpoints');
    const gameSummary = await source.summary('fixture', [games.nodeId]);
    const checkpointSummary = await source.summary('fixture', [checkpoints.nodeId]);
    const first = deferred<SelectionSummary>(), second = deferred<SelectionSummary>();
    const requests: string[][] = [];
    vi.spyOn(source, 'summary').mockImplementation((_scan, ids) => {
      requests.push([...ids]); return requests.length === 1 ? first.promise : second.promise;
    });
    const reveal = vi.spyOn(workspace, 'reveal').mockResolvedValue();
    const oldClick = vi.fn();
    const older = workspace.select(games.nodeId).then(active => { if (active) oldClick(); });
    expect(workspace.state.summary).toBeNull();
    const newer = workspace.select(checkpoints.nodeId);
    expect(workspace.state.summary).toBeNull();
    second.resolve(checkpointSummary); await newer;
    first.resolve(gameSummary); await older;
    expect(requests).toEqual([[games.nodeId], [checkpoints.nodeId]]);
    expect(workspace.state.summary?.logicalTotalBytes).toBe('168000000000');
    expect(workspace.state.primaryId).toBe(checkpoints.nodeId);
    expect(reveal).toHaveBeenCalledExactlyOnceWith(checkpoints.nodeId);
    expect(oldClick).not.toHaveBeenCalled();
  });

  it('does not revive an in flight selection after clear or surface its late error', async () => {
    const {source, workspace, find} = await ready();
    const pending = deferred<SelectionSummary>();
    const empty = await source.summary('fixture', []);
    vi.spyOn(source, 'summary').mockImplementation((_scan, ids) => ids.length ? pending.promise : Promise.resolve(empty));
    const selecting = workspace.select(find('Games').nodeId);
    workspace.clear();
    pending.reject({code: 'NODE_NOT_FOUND', message: 'old reply'});
    expect(await selecting).toBe(false);
    expect(workspace.state.selectedIds).toEqual([]);
    expect(workspace.state.summary?.count).toBe('0');
    expect(workspace.state.error).toBeNull();
  });

  it('keeps the clicked node IDs and revision immutable while discarding a superseded plan', async () => {
    const pending = [deferred<OperationPlan>(), deferred<OperationPlan>()];
    const requests: OperationRequest[] = [];
    const source = {plan: vi.fn((request: OperationRequest) => { requests.push(request); return pending[requests.length - 1].promise; })};
    const preparation = new OperationPreparation();
    const request: OperationRequest = {scanId: 's1', nodeIds: ['a'], expectedRevision: 9, kind: 'rename', newName: 'a-new'};
    const old = preparation.run(source, request);
    request.nodeIds[0] = 'b'; request.expectedRevision = 10; request.newName = 'b-new';
    const current = preparation.run(source, request);
    const plan = (id: string): OperationPlan => ({planId: id, scanId: 's1', revision: 10, kind: 'rename', items: [], logicalTotalBytes: '0', expiresAt: '', warnings: []});
    pending[1].resolve(plan('current'));
    expect((await current)?.planId).toBe('current');
    pending[0].resolve(plan('old'));
    expect(await old).toBeNull();
    expect(requests[0]).toEqual({scanId: 's1', nodeIds: ['a'], expectedRevision: 9, kind: 'rename', newName: 'a-new'});
    expect(requests[1].nodeIds).toEqual(['b']);
  });

  it('ignores a plan error after its dialog closes', async () => {
    const pending = deferred<OperationPlan>(), preparation = new OperationPreparation();
    const request = preparation.run({plan: () => pending.promise}, {scanId: 's1', nodeIds: ['a'], expectedRevision: 1, kind: 'trash'});
    preparation.invalidate(); pending.reject({code: 'SOURCE_CHANGED', message: 'closed dialog'});
    expect(await request).toBeNull();
  });
});
