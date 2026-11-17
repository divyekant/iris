// The proposals of Iris for the brief (card R6-01): the newest proposal with its members, and the count of all.
import { get, type MatterList, type MatterView } from "./api";

export type Proposed = { view: MatterView; count: number };

/** The list and the matter are two requests, so the matter can change between them. Try the list again this many times. */
const TRIES = 3;

/**
 * The newest proposal that the engine gives as proposed now, and the count of
 * all proposals, or null when there is none. If the newest proposal goes (404)
 * or is decided (another tab) between the two requests, load the list again.
 */
export async function loadProposed(): Promise<Proposed | null> {
  for (let i = 0; i < TRIES; i++) {
    const list = await get<MatterList>("matters");
    const first = list.proposed[0];
    if (!first) return null;
    try {
      const view = await get<MatterView>(`matters/${encodeURIComponent(first.id)}`);
      if (view.matter.status === "proposed") return { view, count: list.proposed.length };
    } catch (e) {
      if ((e as { status?: number }).status !== 404) throw e;
    }
  }
  return null;
}
