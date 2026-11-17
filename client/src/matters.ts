// The proposals of Iris for the brief (card R6-01): the newest proposal with its members, and the count of all.
import { get, type MatterList, type MatterView } from "./api";

export type Proposed = { view: MatterView; count: number };

/** The newest proposal and the count of all proposals, or null when there is none. */
export async function loadProposed(): Promise<Proposed | null> {
  const list = await get<MatterList>("matters");
  const first = list.proposed[0];
  if (!first) return null;
  return { view: await get<MatterView>(`matters/${encodeURIComponent(first.id)}`), count: list.proposed.length };
}
