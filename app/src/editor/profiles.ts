import type { Placement } from "../core/store";
import type { DocKind } from "../core/types";

export interface ToolButton {
  id: string;
  /** Tool implementation id (several buttons may share one, e.g. VCC/GND). */
  tool: string;
  label: string;
  icon: string;
  keys: string;
  preset?: Partial<Placement>;
  /** Hidden from the Active Bar (menu/shortcut only). */
  menuOnly?: boolean;
}

const SELECT: ToolButton = { id: "select", tool: "select", label: "Select", icon: "select", keys: "S S" };

export const TOOLS: Record<DocKind, ToolButton[]> = {
  schematic: [
    SELECT,
    { id: "wire", tool: "wire", label: "Wire", icon: "wire", keys: "P W" },
    { id: "bus", tool: "bus", label: "Bus", icon: "bus", keys: "P B" },
    { id: "busEntry", tool: "busEntry", label: "Bus Entry", icon: "busEntry", keys: "P U", menuOnly: true },
    { id: "label", tool: "label", label: "Net Label", icon: "label", keys: "P N" },
    { id: "globalLabel", tool: "globalLabel", label: "Global Label", icon: "globalLabel", keys: "P K" },
    { id: "port", tool: "port", label: "Port (Hierarchical Label)", icon: "port", keys: "P H" },
    { id: "vcc", tool: "power", label: "Power Port (VCC)", icon: "vcc", keys: "P V", preset: { powerNet: "VCC", powerStyle: "bar", rotation: 90 } },
    { id: "gnd", tool: "power", label: "Power Port (GND)", icon: "gnd", keys: "P G", preset: { powerNet: "GND", powerStyle: "ground", rotation: 270 } },
    { id: "part", tool: "part", label: "Part (from Components panel)", icon: "part", keys: "P P" },
    { id: "junction", tool: "junction", label: "Junction", icon: "junction", keys: "P J" },
    { id: "noConnect", tool: "noConnect", label: "No-Connect", icon: "noConnect", keys: "P X" },
    { id: "sheet", tool: "sheet", label: "Sheet Symbol", icon: "sheet", keys: "P S" },
    { id: "text", tool: "text", label: "Text", icon: "text", keys: "P T" },
    { id: "line", tool: "line", label: "Line", icon: "line", keys: "P L", menuOnly: true },
    { id: "rect", tool: "rect", label: "Rectangle", icon: "rect", keys: "P R", menuOnly: true },
    { id: "circle", tool: "circle", label: "Circle", icon: "circle", keys: "P C", menuOnly: true },
    { id: "arc", tool: "arc", label: "Arc", icon: "arc", keys: "P A", menuOnly: true },
  ],
  symbol: [
    SELECT,
    { id: "pin", tool: "pin", label: "Pin", icon: "pin", keys: "P P" },
    { id: "line", tool: "line", label: "Line", icon: "line", keys: "P L" },
    { id: "rect", tool: "rect", label: "Rectangle (body)", icon: "rect", keys: "P R", preset: { fill: "background" } },
    { id: "circle", tool: "circle", label: "Circle", icon: "circle", keys: "P C" },
    { id: "arc", tool: "arc", label: "Arc", icon: "arc", keys: "P A" },
    { id: "polyline", tool: "polyline", label: "Polyline", icon: "polyline", keys: "P Y" },
    { id: "text", tool: "text", label: "Text", icon: "text", keys: "P T" },
  ],
  footprint: [
    SELECT,
    { id: "pad", tool: "pad", label: "Pad", icon: "pad", keys: "P P" },
    { id: "measure", tool: "measure", label: "Measure Distance", icon: "measure", keys: "R M", menuOnly: true },
    { id: "line", tool: "line", label: "Line", icon: "line", keys: "P L" },
    { id: "rect", tool: "rect", label: "Rectangle", icon: "rect", keys: "P R", preset: { fill: "none" } },
    { id: "circle", tool: "circle", label: "Circle", icon: "circle", keys: "P C", preset: { fill: "none" } },
    { id: "arc", tool: "arc", label: "Arc", icon: "arc", keys: "P A" },
    { id: "polyline", tool: "polyline", label: "Polyline", icon: "polyline", keys: "P Y" },
    { id: "text", tool: "text", label: "Text", icon: "text", keys: "P T" },
  ],
  pcb: [
    SELECT,
    { id: "route", tool: "route", label: "Interactive Routing", icon: "track", keys: "P T" },
    { id: "diffPair", tool: "diffPair", label: "Differential Pair Routing", icon: "diffPair", keys: "P I" },
    { id: "via", tool: "via", label: "Via", icon: "via", keys: "P V" },
    { id: "zone", tool: "zone", label: "Polygon Pour", icon: "zone", keys: "P G" },
    { id: "keepout", tool: "keepout", label: "Keep-out Area", icon: "keepout", keys: "P K" },
    { id: "footprint", tool: "footprint", label: "Component (footprint)", icon: "part", keys: "P C" },
    { id: "outline", tool: "outline", label: "Board Outline", icon: "outline", keys: "P O" },
    { id: "reshape", tool: "reshape", label: "Reshape Polygon", icon: "reshape", keys: "E R" },
    { id: "polygonCut", tool: "polygonCut", label: "Cut Polygon", icon: "polygonCut", keys: "E C" },
    { id: "polygonSlice", tool: "polygonSlice", label: "Slice Polygon", icon: "polygonSlice", keys: "E S", menuOnly: true },
    { id: "measure", tool: "measure", label: "Measure Distance", icon: "measure", keys: "R M", menuOnly: true },
    { id: "dimension", tool: "dimension", label: "Dimension", icon: "dimension", keys: "P D" },
    { id: "line", tool: "line", label: "Line", icon: "line", keys: "P L" },
    { id: "arc", tool: "arc", label: "Arc", icon: "arc", keys: "P A" },
    { id: "text", tool: "text", label: "String (text)", icon: "text", keys: "P S" },
    { id: "rect", tool: "rect", label: "Rectangle", icon: "rect", keys: "P R", preset: { fill: "none" }, menuOnly: true },
    { id: "circle", tool: "circle", label: "Circle", icon: "circle", keys: "P E", preset: { fill: "none" }, menuOnly: true },
  ],
};

export function toolButton(kind: DocKind, id: string): ToolButton | undefined {
  return TOOLS[kind].find((t) => t.id === id);
}
