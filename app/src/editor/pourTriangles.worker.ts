import { triangulateWithHoles } from "./shapes";
import type { FillRequest, FillResponse } from "./pourTriangles";

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<FillRequest>) => void) | null;
  postMessage(response: FillResponse, transfer: Transferable[]): void;
};
scope.onmessage = ({ data: { id, polygon } }) => {
  try {
    const triangles = new Float64Array(triangulateWithHoles(polygon.outer, polygon.holes));
    scope.postMessage({ id, triangles: triangles.buffer }, [triangles.buffer]);
  } catch {
    scope.postMessage({ id, error: "Copper fill preparation failed" }, []);
  }
};
