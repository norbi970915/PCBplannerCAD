import { useEffect, useRef, useState, type ReactNode } from "react";
import { create } from "zustand";
import { Button } from "@/components/ui/button";
import { Dialog as UiDialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

interface ModalState {
  content: ReactNode | null;
  show: (content: ReactNode) => void;
  close: () => void;
}

export const useModal = create<ModalState>()((set) => ({
  content: null,
  show: (content) => set({ content }),
  close: () => set({ content: null }),
}));

/** Renders whichever dialog is open; closing (Esc, backdrop, ×) clears it. */
export function ModalHost() {
  const content = useModal((s) => s.content);
  return (
    <UiDialog open={!!content} onOpenChange={(open) => !open && useModal.getState().close()}>
      {content}
    </UiDialog>
  );
}

/** Standard dialog chrome used by every modal in the app. */
export function Dialog({ title, description, children, footer, wide }: { title: string; description?: string; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  return (
    <DialogContent className={cn("gap-0 p-0", wide ? "sm:max-w-[880px]" : "sm:max-w-[440px]")} onPointerDownOutside={(e) => e.preventDefault()}>
      <DialogHeader className="border-b border-line bg-panel-head px-4 py-3">
        <DialogTitle className="text-[13px]">{title}</DialogTitle>
        {description && <DialogDescription className="text-xs">{description}</DialogDescription>}
      </DialogHeader>
      <div className="max-h-[70vh] overflow-auto px-4 py-3">{children}</div>
      {footer && <DialogFooter className="border-t border-line bg-panel-head px-4 py-2.5">{footer}</DialogFooter>}
    </DialogContent>
  );
}

interface PromptField {
  name: string;
  label: string;
  value: string;
}

function PromptForm({ title, fields, ok, resolve }: { title: string; fields: PromptField[]; ok: string; resolve: (v: Record<string, string> | null) => void }) {
  const [values, setValues] = useState(Object.fromEntries(fields.map((f) => [f.name, f.value])));
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const t = window.setTimeout(() => first.current?.select(), 30);
    return () => window.clearTimeout(t);
  }, []);
  const submit = () => {
    resolve(values);
    useModal.getState().close();
  };
  return (
    <Dialog
      title={title}
      footer={
        <>
          <Button variant="outline" onClick={() => (resolve(null), useModal.getState().close())}>
            Cancel
          </Button>
          <Button onClick={submit}>{ok}</Button>
        </>
      }
    >
      <div className="grid gap-3">
        {fields.map((f, i) => (
          <div key={f.name} className="grid gap-1.5">
            <Label htmlFor={`prompt-${f.name}`} className="text-xs text-muted-foreground">
              {f.label}
            </Label>
            <Input
              id={`prompt-${f.name}`}
              ref={i === 0 ? first : undefined}
              value={values[f.name]}
              onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && submit()}
            />
          </div>
        ))}
      </div>
    </Dialog>
  );
}

/** Asks for one or more text values; resolves null when cancelled. */
export function promptFields(title: string, fields: PromptField[], ok = "OK"): Promise<Record<string, string> | null> {
  return new Promise((resolve) => {
    let settled = false;
    const done = (v: Record<string, string> | null) => {
      if (!settled) {
        settled = true;
        resolve(v);
      }
    };
    useModal.getState().show(<PromptForm title={title} fields={fields} ok={ok} resolve={done} />);
    const unsub = useModal.subscribe((s) => {
      if (!s.content) {
        unsub();
        done(null);
      }
    });
  });
}

export async function promptText(title: string, label: string, value = ""): Promise<string | null> {
  const r = await promptFields(title, [{ name: "v", label, value }]);
  const v = r?.v.trim();
  return v ? v : null;
}
