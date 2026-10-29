import type { PcbDragPlan, RouteMode } from "../core/types";

/** A failed plan never reaches the command/history layer. */
export function moveRejection(plan: PcbDragPlan, mode?: RouteMode): string | null {
  if (plan.message) return plan.message;
  if (plan.failure === "unroutable") return "No clear route to that position. Choose another location or change the move mode.";
  if (plan.failure === "locked" || (plan.blocked && !plan.replace.length && !plan.add.length && !plan.delete.length)) return "Move blocked by locked items or connected copper. Unlock them or choose Disconnect.";
  if (plan.blocked && mode && mode !== "ignore") return "No clear route to that position. Choose another location or change the move mode.";
  return null;
}
