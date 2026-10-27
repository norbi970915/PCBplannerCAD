import type { LayerId, StackLayer } from "../core/types";
import { NM_PER_MM } from "../core/units";
import { VIA_KIND_LABEL, viaKind, viaSpan } from "./vias";

export { MICROVIA_MAX_DRILL, VIA_KIND_LABEL, viaKind, viaSpan, type ViaKind } from "./vias";

export interface StackupVia {
  label: string;
  from: LayerId;
  to: LayerId;
  diameter: number;
  drill: number;
  /** Vias of this kind placed on the board. */
  count: number;
  /** False for placed vias that match no defined via type. */
  defined: boolean;
}

const BAND: Record<StackLayer["kind"], number> = { copper: 9, dielectric: 0, solderMask: 6, silkscreen: 5, paste: 4 };
const LABEL_W = 132;
const COL_W = 54;

/** Cross-section of the layer stack with every via drawn across the layers it connects. */
export function StackupView({ layers, vias, mask, silk, highlight, onHover }: {
  layers: StackLayer[];
  vias: StackupVia[];
  mask: string;
  silk: string;
  highlight?: number | null;
  onHover?: (i: number | null) => void;
}) {
  const copper = layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => l.layer as LayerId);
  const thickest = Math.max(1, ...layers.filter((l) => l.kind === "dielectric").map((l) => l.thickness));
  // Dielectrics scale with thickness (with a readable minimum); thin films get fixed bands.
  const heights = layers.map((l) => (l.kind === "dielectric" ? 16 + 34 * (l.thickness / thickest) : BAND[l.kind]));
  const tops: number[] = [];
  let y = 8;
  for (const h of heights) { tops.push(y); y += h + 1; }
  const stackBottom = y;
  const cols = Math.max(1, vias.length);
  const width = LABEL_W + 16 + cols * COL_W + 70;
  const height = stackBottom + 56;
  const fill = (l: StackLayer) =>
    l.kind === "copper" ? "var(--ui-copper)" : l.kind === "dielectric" ? "var(--ui-laminate)" : l.kind === "solderMask" ? mask : l.kind === "silkscreen" ? silk : "#9aa0a8";
  const band = (id: LayerId) => layers.findIndex((l) => l.kind === "copper" && l.layer === id);
  const bandsX = LABEL_W, bandsW = width - LABEL_W - 66;
  const total = layers.reduce((s, l) => s + l.thickness, 0);

  return (
    // Natural size (1 unit = 1 px) so labels stay readable; the container scrolls when there are many via columns.
    <svg viewBox={`0 0 ${width} ${height}`} width={width} height={height} className="max-w-none shrink-0 select-none text-muted-foreground" role="img" aria-label="Layer stack cross-section with vias">
      {layers.map((l, i) => (
        <g key={i}>
          <rect x={bandsX} y={tops[i]} width={bandsW} height={heights[i]} fill={fill(l)} opacity={l.kind === "dielectric" ? 0.55 : 0.95} rx={1} />
          {(l.kind === "copper" || l.kind === "dielectric") && (
            <text x={LABEL_W - 6} y={tops[i] + heights[i] / 2 + 3.5} textAnchor="end" fontSize={l.kind === "copper" ? 10 : 9} fill="currentColor" fontWeight={l.kind === "copper" ? 600 : 400}>
              {l.name}
            </text>
          )}
          {l.thickness > 0 && (l.kind === "copper" || l.kind === "dielectric") && (
            <text x={bandsX + bandsW + 6} y={tops[i] + heights[i] / 2 + 3.5} fontSize={9} fill="currentColor" fontFamily="Consolas, monospace">
              {(l.thickness / NM_PER_MM).toFixed(3)}
            </text>
          )}
        </g>
      ))}
      {vias.map((v, i) => {
        const a = band(v.from), b = band(v.to);
        if (a < 0 || b < 0) return null;
        const [ta, tb] = a <= b ? [a, b] : [b, a];
        const top = tops[ta], bottom = tops[tb] + heights[tb];
        const cx = bandsX + 18 + i * COL_W + COL_W / 2;
        const kind = viaKind(copper, v.from, v.to, v.drill);
        // Barrel width follows the drill relative to a 0.3 mm through via (clamped for legibility).
        const hole = Math.max(4, Math.min(16, (v.drill / 300_000) * 10));
        const pad = Math.max(hole + 6, Math.min(26, (v.diameter / 600_000) * 20));
        const lit = highlight === i;
        const spanned = viaSpan(copper, v.from, v.to);
        return (
          <g key={i} onMouseEnter={() => onHover?.(i)} onMouseLeave={() => onHover?.(null)} style={{ cursor: "default" }}>
            {lit && <rect x={cx - COL_W / 2 + 2} y={top - 4} width={COL_W - 4} height={bottom - top + 8} rx={3} fill="var(--ui-accent)" opacity={0.18} />}
            {kind === "micro" ? (
              // Laser cone: wide at the outer surface, narrowing toward the landing layer.
              <polygon
                points={tb !== band(copper[copper.length - 1])
                  ? `${cx - pad / 2},${top} ${cx + pad / 2},${top} ${cx + hole / 2},${bottom} ${cx - hole / 2},${bottom}`
                  : `${cx - hole / 2},${top} ${cx + hole / 2},${top} ${cx + pad / 2},${bottom} ${cx - pad / 2},${bottom}`}
                fill="var(--ui-copper)" stroke="#00000055" strokeWidth={0.6}
              />
            ) : (
              <>
                <rect x={cx - hole / 2 - 1.6} y={top} width={hole + 3.2} height={bottom - top} fill="var(--ui-copper)" />
                <rect x={cx - hole / 2} y={top} width={hole} height={bottom - top} fill="#16181c" />
              </>
            )}
            {kind !== "micro" && spanned.map((id) => {
              const k = band(id);
              return <rect key={id} x={cx - pad / 2} y={tops[k]} width={pad} height={heights[k]} fill="var(--ui-copper)" stroke="#00000055" strokeWidth={0.6} />;
            })}
            <text x={cx} y={stackBottom + 12} textAnchor="middle" fontSize={9} fontWeight={600} fill={v.defined ? "currentColor" : "var(--err-line, #e5484d)"}>
              {VIA_KIND_LABEL[kind]}
            </text>
            <text x={cx} y={stackBottom + 23} textAnchor="middle" fontSize={8.5} fill="currentColor" fontFamily="Consolas, monospace">
              {(v.diameter / NM_PER_MM).toFixed(2)}/{(v.drill / NM_PER_MM).toFixed(2)}
            </text>
            <text x={cx} y={stackBottom + 34} textAnchor="middle" fontSize={8.5} fill="currentColor">
              {v.count ? `×${v.count}` : v.defined ? "unused" : ""}
            </text>
            <title>{`${v.label}: ${VIA_KIND_LABEL[kind]} via, ${spanned.length} copper layers, Ø${(v.diameter / NM_PER_MM).toFixed(2)} mm / drill ${(v.drill / NM_PER_MM).toFixed(2)} mm${v.count ? `, ${v.count} on the board` : ""}${v.defined ? "" : " (not a defined via type)"}`}</title>
          </g>
        );
      })}
      <text x={bandsX} y={height - 3} fontSize={9} fill="currentColor">{`${copper.length} copper layers · ${(total / NM_PER_MM).toFixed(3)} mm`}</text>
    </svg>
  );
}
