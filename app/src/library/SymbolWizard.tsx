import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, type SymbolSpec } from "../core/api";
import { report } from "../core/actions";
import { Dialog, useModal } from "../shell/modal";

const TYPES = ["input", "output", "bidirectional", "triState", "passive", "powerIn", "powerOut", "openCollector", "openEmitter", "noConnect", "unspecified"];
const SIDES = ["left", "right", "top", "bottom"];

const EXAMPLE = `1 VIN powerIn left
2 GND powerIn bottom
3 EN input left
4 NC noConnect right
5 VOUT powerOut right`;

/** Parses "number name [type] [side]" lines. */
function parsePins(text: string): SymbolSpec["pins"] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .map((line, i) => {
      const parts = line.split(/[\s,;\t]+/);
      const [number = String(i + 1), name = number] = parts;
      const type = parts.find((p, j) => j >= 2 && TYPES.includes(p)) ?? "passive";
      const side = parts.find((p, j) => j >= 2 && SIDES.includes(p)) ?? (i % 2 ? "right" : "left");
      return { number, name, electrical: type, side };
    });
}

export function SymbolWizard({ libId, onDone }: { libId: string; onDone: (name: string) => void }) {
  const [name, setName] = useState("IC");
  const [prefix, setPrefix] = useState("U");
  const [text, setText] = useState(EXAMPLE);
  const [makeComponent, setMakeComponent] = useState(true);
  const pins = parsePins(text);
  const count = (side: string) => pins.filter((p) => p.side === side).length;

  const create = async () => {
    try {
      const symbolName = await api.symbolWizard(libId, { name, prefix, pins });
      if (makeComponent) {
        await api.putComponent(
          libId,
          { name: symbolName, description: "", symbol: symbolName, footprints: [], pinMap: [], parameters: [{ name: "Value", value: symbolName }] },
          null,
        );
      }
      useModal.getState().close();
      onDone(symbolName);
    } catch (e) {
      report(e);
    }
  };

  return (
    <Dialog
      title="Symbol wizard"
      description="Paste a pin table to generate a rectangular IC symbol on the 1.27 mm grid."
      wide
      footer={
        <>
          <Button variant="outline" onClick={() => useModal.getState().close()}>
            Cancel
          </Button>
          <Button disabled={!pins.length || !name.trim()} onClick={() => void create()}>
            Create symbol
          </Button>
        </>
      }
    >
      <div className="grid gap-3 text-xs">
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1.5">
            <Label className="text-xs text-muted-foreground">Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid gap-1.5">
            <Label className="text-xs text-muted-foreground">Designator prefix</Label>
            <Input value={prefix} onChange={(e) => setPrefix(e.target.value)} />
          </div>
        </div>
        <div className="grid gap-1.5">
          <Label className="text-xs text-muted-foreground">
            Pins — one per line: <code className="font-mono">number name [type] [side]</code>
          </Label>
          <Textarea className="min-h-56 font-mono text-xs" value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} />
          <p className="text-[11px] text-faint">
            Types: {TYPES.join(", ")} · Sides: {SIDES.join(", ")}
          </p>
        </div>
        <div className="flex items-center justify-between">
          <span className="font-mono text-[11px] text-muted-foreground">
            {pins.length} pins · L {count("left")} · R {count("right")} · T {count("top")} · B {count("bottom")}
          </span>
          <label className="flex items-center gap-2">
            <Checkbox checked={makeComponent} onCheckedChange={(v) => setMakeComponent(v === true)} /> Also create a component using this symbol
          </label>
        </div>
      </div>
    </Dialog>
  );
}
