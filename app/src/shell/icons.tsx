import type { ReactNode } from "react";
import {
  BookOpen,
  Box,
  Check,
  Circle,
  CircuitBoard,
  Cpu,
  FileCode2,
  FolderOpen,
  Layers,
  Maximize,
  MousePointer2,
  Plus,
  RectangleHorizontal,
  Redo2,
  Save,
  ShieldCheck,
  Spline,
  Scissors,
  Split,
  Type,
  Undo2,
  Wand2,
  X,
} from "lucide-react";

// Generic actions use lucide; EDA-specific tools are drawn here in the same
// stroke style so the toolbar reads as one set.

const lc = { size: 15, strokeWidth: 1.75 } as const;

const Svg = ({ children }: { children: ReactNode }) => (
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    {children}
  </svg>
);

const dot = (cx: number, cy: number, r = 1.3) => <circle cx={cx} cy={cy} r={r} fill="currentColor" stroke="none" />;

export const Icons: Record<string, ReactNode> = {
  // generic
  select: <MousePointer2 {...lc} />,
  fit: <Maximize {...lc} />,
  undo: <Undo2 {...lc} />,
  redo: <Redo2 {...lc} />,
  singleLayer: <Layers {...lc} />,
  folder: <FolderOpen {...lc} />,
  board: <CircuitBoard {...lc} />,
  schematic: <FileCode2 {...lc} />,
  library: <BookOpen {...lc} />,
  plus: <Plus {...lc} />,
  check: <Check {...lc} />,
  save: <Save {...lc} />,
  close: <X {...lc} />,
  wizard: <Wand2 {...lc} />,
  erc: <ShieldCheck {...lc} />,
  text: <Type {...lc} />,
  rect: <RectangleHorizontal {...lc} />,
  circle: <Circle {...lc} />,
  component: <Cpu {...lc} />,
  model3d: <Box {...lc} />,
  polyline: <Spline {...lc} />,
  polygonCut: <Scissors {...lc} />,
  polygonSlice: <Split {...lc} />,

  // EDA tools
  line: (
    <Svg>
      <path d="M3 13L13 3" />
      {dot(3, 13)}
      {dot(13, 3)}
    </Svg>
  ),
  wire: (
    <Svg>
      <path d="M2 12h6V4h6" />
      {dot(8, 12, 1)}
    </Svg>
  ),
  bus: (
    <Svg>
      <path d="M2 11h12" strokeWidth="3" />
      <path d="M5 11l-2-3M9 11l-2-3" />
    </Svg>
  ),
  busEntry: (
    <Svg>
      <path d="M3 13h10" strokeWidth="2.6" />
      <path d="M6 13l4-6h4" />
    </Svg>
  ),
  label: (
    <Svg>
      <path d="M1.5 12h13" />
      <path d="M3 9.5V4h2.5l3 5.5V4" />
    </Svg>
  ),
  globalLabel: (
    <Svg>
      <path d="M1 8l3-3.5h10v7H4z" />
      <path d="M7 8h4" />
    </Svg>
  ),
  port: (
    <Svg>
      <path d="M1 8l2.5-3h3.5v6H3.5z" />
      <path d="M9 8h6" />
    </Svg>
  ),
  vcc: (
    <Svg>
      <path d="M8 15V6M4 6h8" />
      <path d="M5.5 3.5L8 1l2.5 2.5" strokeWidth="1.2" />
    </Svg>
  ),
  gnd: (
    <Svg>
      <path d="M8 1v7M3 8h10M5 10.5h6M7 13h2" />
    </Svg>
  ),
  part: (
    <Svg>
      <rect x="4" y="3" width="8" height="10" rx="1" />
      <path d="M1 6h3M1 10h3M12 6h3M12 10h3" />
    </Svg>
  ),
  junction: (
    <Svg>
      {dot(8, 8, 2.6)}
      <path d="M1 8h14M8 8v7" />
    </Svg>
  ),
  noConnect: (
    <Svg>
      <path d="M4 4l8 8M12 4l-8 8" />
    </Svg>
  ),
  sheet: (
    <Svg>
      <rect x="3" y="2.5" width="10" height="11" rx="1" />
      <path d="M1 6h2M1 10h2M13 8h2" />
    </Svg>
  ),
  arc: (
    <Svg>
      <path d="M2.5 12A6 6 0 0113.5 6" />
    </Svg>
  ),
  pin: (
    <Svg>
      <rect x="8" y="3" width="6" height="10" />
      <path d="M2 8h6" />
      {dot(2, 8)}
    </Svg>
  ),
  pad: (
    <Svg>
      <rect x="3" y="4" width="10" height="8" rx="2" fill="currentColor" stroke="none" opacity="0.85" />
    </Svg>
  ),
  track: (
    <Svg>
      <path d="M2 13h4l6-6V2" strokeWidth="2.4" />
    </Svg>
  ),
  via: (
    <Svg>
      <circle cx="8" cy="8" r="5" />
      <circle cx="8" cy="8" r="2" fill="currentColor" stroke="none" />
    </Svg>
  ),
  zone: (
    <Svg>
      <path d="M2 3h9l3 4v6H2z" fill="currentColor" fillOpacity="0.25" />
    </Svg>
  ),
  measure: (
    <Svg>
      <path d="M1.5 10.5l9-9 4 4-9 9z" />
      <path d="M4.5 7.5l1.5 1.5M6.5 5.5l1 1M8.5 3.5l1.5 1.5" />
    </Svg>
  ),
  reshape: (
    <Svg>
      <path d="M2 13V3h7l5 5v5z" fill="currentColor" fillOpacity="0.2" />
      <path d="M6 3l2 4h6" strokeDasharray="1.5 1.2" />
      <rect x="4.8" y="1.8" width="2.4" height="2.4" fill="currentColor" stroke="none" />
      <rect x="12.8" y="5.8" width="2.4" height="2.4" fill="currentColor" stroke="none" />
    </Svg>
  ),
  keepout: (
    <Svg>
      <rect x="2.5" y="2.5" width="11" height="11" strokeDasharray="2 1.5" />
      <path d="M5 5l6 6M11 5l-6 6" />
    </Svg>
  ),
  outline: (
    <Svg>
      <path d="M2 4l3-2h9v9l-2 3H2z" strokeDasharray="2.5 1.5" />
    </Svg>
  ),
  footprint: (
    <Svg>
      <rect x="2" y="5" width="4" height="6" rx="1" />
      <rect x="10" y="5" width="4" height="6" rx="1" />
    </Svg>
  ),
  symbol: (
    <Svg>
      <path d="M4 3v10l9-5z" />
      <path d="M1 6h3M1 10h3M13 8h2" />
    </Svg>
  ),
  diffPair: (
    <Svg>
      <path d="M1 6h5l3-3h6M1 10h5l3 3h6" />
    </Svg>
  ),
  tune: (
    <Svg>
      <path d="M1 8h2V4h3v8h3V4h3v4h3" />
    </Svg>
  ),
};
