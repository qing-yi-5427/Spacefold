import type { OperationPlan, OperationRequest } from './contracts';
import type { DataSource } from './data';

/** An operation form owns its request; closing or editing it invalidates late replies. */
export class OperationPreparation {
  private sequence = 0;
  invalidate() { this.sequence++; }
  async run(source: Pick<DataSource, 'plan'>, request: OperationRequest): Promise<OperationPlan|null> {
    const sequence = ++this.sequence;
    const bound = {...request, nodeIds: [...request.nodeIds]};
    try {
      const plan = await source.plan(bound);
      return sequence === this.sequence ? plan : null;
    } catch (error) {
      if (sequence !== this.sequence) return null;
      throw error;
    }
  }
}
