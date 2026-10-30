import { useEffect, useState } from "react";
import { api, type WizardFamily, type WizardParam, type WizardSpec, type WizardGoals } from "../core/api";
import { report } from "../core/actions";
import type { Footprint } from "../core/types";
import { PreviewCanvas } from "../editor/PreviewCanvas";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Dialog, useModal } from "../shell/modal";

/** Editable footprint geometry with explicit user-selected land goals. */
export function FootprintWizard({ libId, onDone }: { libId: string; onDone: (name: string) => void }) {
  const [pending, setPending] = useState(true);
  const [busy, setBusy] = useState(false);
  const [goals, setGoals] = useState<WizardGoals>({ toe: 0.10, heel: 0, side: 0 });
  const [families, setFamilies] = useState<WizardFamily[]>([]);
  const [familyId, setFamilyId] = useState("chip");
  const [spec, setSpec] = useState<WizardSpec | null>(null);
  const [preview, setPreview] = useState<Footprint | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [nameEdited, setNameEdited] = useState(false);

  useEffect(() => {
    api
      .wizardFamilies()
      .then((f) => {
        setFamilies(f);
        const first = f.find((x) => x.id === familyId) ?? f[0];
        if (first) setSpec(structuredClone(first.defaultSpec));
      })
      .catch(report);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const family = families.find((f) => f.id === familyId);

  useEffect(() => {
    if (!spec) return;
    let cancelled = false;
    setPending(true); setPreview(null);
    const t = window.setTimeout(() => {
      api
        .wizardPreview(spec, goals)
        .then((fp) => {
          if (cancelled) return;
          setPending(false); setPreview(fp);
          setError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setPending(false); setPreview(null);
          setError(String(e));
        });
      if (!nameEdited) api.wizardDefaultName(spec).then(value => { if (!cancelled) setName(value); }).catch(() => {});
    }, 120);
    return () => { cancelled = true; window.clearTimeout(t); };
  }, [spec, nameEdited, goals]);

  const pickFamily = (id: string) => {
    setFamilyId(id);
    const f = families.find((x) => x.id === id);
    if (f) setSpec(structuredClone(f.defaultSpec));
    setNameEdited(false);
  };

  const setParam = (key: string, value: unknown) => spec && setSpec({ ...spec, [key]: value });

  const generate = async () => {
    if (!spec || pending || busy || !preview) return;
    setBusy(true);
    try {
      const n = await api.wizardGenerate(libId, spec, name.trim() || null, goals);
      useModal.getState().close();
      onDone(n);
    } catch (e) {
      report(e);
    } finally { setBusy(false); }
  };

  const pads = preview?.items.filter((i) => i.kind === "pad").length ?? 0;

  return (
    <Dialog
      title="Footprint wizard"
      description="Editable footprint estimates. Verify the component drawing and fabrication process; no standards compliance is claimed."
      wide
      busy={busy}
      footer={
        <>
          <Button variant="outline" disabled={busy} onClick={() => useModal.getState().close()}>
            Cancel
          </Button>
          <Button disabled={!preview || pending || busy} onClick={() => void generate()}>
            Add to Library
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-[340px_minmax(0,1fr)] gap-4 text-xs">
        <div className="grid content-start gap-0.5">
          <label className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 py-0.5">
            <span>Family</span>
            <NativeSelect value={familyId} onChange={(e) => pickFamily(e.target.value)}>
              {families.map((f) => (
                <NativeSelectOption key={f.id} value={f.id}>
                  {f.name}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </label>
          {family && <p className="pb-1 text-[11px] text-muted-foreground">{family.description}</p>}
          {family?.params.map((p) => <ParamInput key={`${familyId}-${p.key}`} p={p} value={spec?.[p.key]} onChange={(v) => setParam(p.key, v)} />)}
          <fieldset className="my-2 grid gap-1 rounded border border-line p-2"><legend className="px-1 text-xs">Land extension goals (mm)</legend><p className="text-[11px] text-muted-foreground">Uniform PCBPlanner Studio starting choices, not a standards table. Some fixed package templates use explicit geometry instead.</p>{(["toe", "heel", "side"] as const).map(key => <label key={key} className="grid grid-cols-[1fr_100px] items-center gap-2 text-xs"><span>{key[0].toUpperCase() + key.slice(1)}</span><Input aria-label={key + " extension (mm)"} type="number" min={key === "toe" ? 0 : -0.5} max={2} step="0.05" value={Number.isFinite(goals[key]) ? goals[key] : ""} onChange={event => setGoals(previous => ({ ...previous, [key]: event.currentTarget.valueAsNumber }))} /></label>)}</fieldset>
          <label className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 py-0.5">
            <span>Name</span>
            <Input
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setNameEdited(true);
              }}
            />
          </label>
        </div>
        <div className="grid min-w-0 content-start gap-1.5">
          <PreviewCanvas items={preview?.items ?? []} kind="footprint" height={340} />
          <div className="text-[11px] text-muted-foreground">{error ? <span className="text-err-line">{error}</span> : `${pads} pads`}</div>
        </div>
      </div>
    </Dialog>
  );
}

function ParamInput({ p, value, onChange }: { p: WizardParam; value: unknown; onChange: (v: unknown) => void }) {
  const label = `${p.label}${p.unit ? ` (${p.unit})` : ""}`;
  const shown = value ?? null;
  if (p.kind === "bool") {
    return (
      <label className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 py-0.5">
        <span>{label}</span>
        <Checkbox checked={Boolean(shown ?? p.default)} onCheckedChange={(v) => onChange(v === true)} className="size-3.5" />
      </label>
    );
  }
  if (p.kind === "choice") {
    return (
      <label className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 py-0.5">
        <span>{label}</span>
        <NativeSelect value={String(shown ?? p.default)} onChange={(e) => onChange(typeof p.default === "number" ? Number(e.target.value) : e.target.value)}>
          {p.choices.map((c) => (
            <NativeSelectOption key={String(c)} value={String(c)}>
              {String(c)}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </label>
    );
  }
  return <TextParam p={p} label={label} value={shown} onChange={onChange} />;
}

/** Free-form text/number input; commits on blur or Enter so partial numbers can be typed. */
function TextParam({ p, label, value, onChange }: { p: WizardParam; label: string; value: unknown; onChange: (v: unknown) => void }) {
  const numeric = p.kind === "number" || p.kind === "integer";
  const listId = p.choices.length ? `wiz-${p.key}` : undefined;
  const [text, setText] = useState(value === null ? "" : String(value));
  useEffect(() => setText(value === null ? "" : String(value)), [value]);
  const commit = () => {
    const t = text.trim();
    if (!t) return onChange(p.optional ? null : p.default);
    if (!numeric) return onChange(text);
    const n = p.kind === "integer" ? parseInt(t, 10) : parseFloat(t);
    if (Number.isFinite(n)) onChange(n);
    else setText(value === null ? "" : String(value));
  };
  return (
    <label className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 py-0.5">
      <span>{label}</span>
      <div className="flex items-center gap-2">
        <Input
          className="font-mono"
          list={listId}
          value={text}
          placeholder={p.optional ? `auto (${String(p.default ?? "")})` : String(p.default ?? "")}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />
        {listId && (
          <datalist id={listId}>
            {p.choices.map((c) => (
              <NativeSelectOption key={String(c)} value={String(c)} />
            ))}
          </datalist>
        )}
      </div>
    </label>
  );
}
