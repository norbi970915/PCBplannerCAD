import { useEffect, useRef, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { cn } from "@/lib/utils";
import { formatLength, parseLength, type Units } from "@/core/units";

/** Text input that commits on Enter/blur and reverts on Escape. */
export function CommitInput({
  value,
  onCommit,
  format = (v) => v,
  parse = (t) => t,
  mono,
  placeholder,
  className,
  list,
}: {
  value: string;
  onCommit: (v: string) => void;
  format?: (v: string) => string;
  parse?: (t: string) => string | null;
  mono?: boolean;
  placeholder?: string;
  className?: string;
  /** id of a <datalist> with suggestions. */
  list?: string;
}) {
  const shown = format(value);
  const [text, setText] = useState(shown);
  const cancelBlur=useRef(false);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    const v = parse(text);
    if (v === null || v === value) setText(format(value));
    else onCommit(v);
  };
  return (
    <Input
      value={text}
      placeholder={placeholder}
      list={list}
      className={cn(mono && "font-mono", className)}
      onChange={(e) => setText(e.target.value)}
      onBlur={()=>{if(cancelBlur.current){cancelBlur.current=false;return;}commit();}}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") {
          cancelBlur.current=true;
          e.preventDefault();e.stopPropagation();
          setText(format(value));
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

/** Length in the current display units ("1.27", "50mil" and "1.27mm" all accepted). */
export function LengthInput({ value, units, onCommit, min }: { value: number; units: Units; onCommit: (nm: number) => void; min?: number }) {
  return (
    <CommitInput
      mono
      value={String(value)}
      format={(v) => formatLength(Number(v), units)}
      parse={(t) => {
        const nm = parseLength(t, units);
        return nm === null || (min !== undefined && nm < min) ? null : String(nm);
      }}
      onCommit={(v) => onCommit(Number(v))}
    />
  );
}

export function NumberInput({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  return (
    <CommitInput
      mono
      value={String(value)}
      parse={(t) => (Number.isFinite(parseFloat(t)) ? String(parseFloat(t)) : null)}
      onCommit={(v) => onCommit(Number(v))}
    />
  );
}

export function Select<T extends string | number>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <NativeSelect value={String(value)} onChange={(e) => onChange(options.find(([v]) => String(v) === e.target.value)![0])}>
      {options.map(([v, l]) => (
        <NativeSelectOption key={String(v)} value={String(v)}>
          {l}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}

export function Check({ value, onChange }: { value: boolean; onChange: (v: boolean) => void }) {
  return <Checkbox checked={value} onCheckedChange={(c) => onChange(c === true)} className="size-3.5" />;
}
