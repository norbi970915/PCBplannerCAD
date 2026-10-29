import { useId, useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { readPreference } from "../../core/preferences";
import { cn } from "@/lib/utils";

/** Scrollable panel body with the panel surface color. */
export function Panel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("flex h-full min-h-0 flex-col overflow-hidden bg-panel text-xs", className)}>{children}</div>;
}

/** 37px heading row (title left, actions right). */
export function PanelHeading({ title, children, className }: { title: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex h-[34px] shrink-0 items-center gap-2 border-b border-line bg-panel-head px-3 text-xs font-semibold", className)}>
      <span className="min-w-0 flex-1 truncate">{title}</span>
      {children && <span className="flex items-center gap-0.5 font-normal">{children}</span>}
    </div>
  );
}

/** Section with an uppercase eyebrow title. */
export function Section({ title, actions, children, className }: { title: string; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn("border-b border-line last:border-b-0", className)}>
      <div className="flex h-7 items-center justify-between px-3 pt-1">
        <span className="eyebrow">{title}</span>
        {actions && <span className="flex items-center gap-0.5">{actions}</span>}
      </div>
      <div className="px-3 pb-2.5">{children}</div>
    </section>
  );
}

/** Opt-in disclosure section. Expansion is a local UI preference, never design data. */
export function CollapsibleSection({title,count,dirty=false,preferenceKey,children}:{title:string;count:number;dirty?:boolean;preferenceKey:string;children:ReactNode}) {
  const id=useId();
  const [expanded,setExpanded]=useState(()=>{try{return readPreference(preferenceKey)==="1";}catch{return false;}});
  const toggle=()=>{const next=!expanded;setExpanded(next);try{localStorage.setItem(preferenceKey,next?"1":"0");}catch{/* Optional UI preference. */}};
  return <section aria-label={title} className="border-b border-line last:border-b-0">
    <button type="button" aria-expanded={expanded} aria-controls={id} onClick={toggle} className="flex h-7 w-full items-center gap-1.5 px-3 text-left hover:bg-hover focus-visible:outline-2 focus-visible:outline-accent-ink">
      <ChevronRight aria-hidden="true" className={cn("size-3 shrink-0 transition-transform",expanded&&"rotate-90")}/>
      <span className="eyebrow min-w-0 flex-1 truncate">{title}</span>
      <span className="rounded bg-secondary px-1.5 font-mono text-[10px] text-muted-foreground">{count}</span>
      {dirty&&<span role="img" aria-label="Unsaved library changes" className="dirty-dot"/>}
    </button>
    <div id={id} hidden={!expanded} className="px-3 pb-2.5">{expanded&&children}</div>
  </section>;
}

/** Label/value row used by property grids. */
export function PropRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid min-h-7 grid-cols-[96px_minmax(0,1fr)] items-center gap-2 py-0.5">
      <span className="truncate text-muted-foreground">{label}</span>
      <span className="min-w-0">{children}</span>
    </label>
  );
}

export function TreeRow({
  depth = 0,
  icon,
  children,
  active,
  onClick,
  trailing,
  title,
}: {
  depth?: number;
  icon?: ReactNode;
  children: ReactNode;
  active?: boolean;
  onClick?: () => void;
  trailing?: ReactNode;
  title?: string;
}) {
  return (
    <div
      title={title}
      onClick={onClick}
      style={{ paddingLeft: 8 + depth * 14 }}
      className={cn(
        "group flex h-[24px] items-center gap-1.5 rounded-[4px] pr-1.5 whitespace-nowrap",
        onClick && "cursor-default hover:bg-hover",
        active && "bg-sel text-ink hover:bg-sel",
      )}
    >
      {icon && <span className="flex shrink-0 text-faint [&_svg]:size-3.5">{icon}</span>}
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {trailing}
    </div>
  );
}

export function EmptyNote({ icon, title, children }: { icon?: ReactNode; title?: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-2 px-6 py-8 text-center text-muted-foreground">
      {icon && <span className="text-faint [&_svg]:size-6">{icon}</span>}
      {title && <div className="text-xs font-semibold text-ink">{title}</div>}
      {children && <div className="text-xs leading-relaxed">{children}</div>}
    </div>
  );
}
