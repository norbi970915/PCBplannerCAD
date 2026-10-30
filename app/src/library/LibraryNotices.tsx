import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, useModal } from "../shell/modal";
import { api } from "../core/api";
import type { Library } from "../core/types";

export function LibraryNotices({ lib, id, readonly, changed }: { lib: Library; id: string; readonly: boolean; changed: () => Promise<void> }) {
  const [text, setText] = useState((lib.notices ?? []).join("\n\n---\n\n"));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const save = async () => {
    setBusy(true); setError("");
    try { await api.libSetNotices(id, text.trim() ? [text] : []); await changed(); useModal.getState().close(); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  return <Dialog title="Library notices" description="Preserve source, author, license and redistribution conditions with this library. Importing does not grant new rights." wide busy={busy} footer={<>
    <Button variant="outline" disabled={busy} onClick={() => useModal.getState().close()}>Close</Button>
    {!readonly && <Button disabled={busy} onClick={() => void save()}>Keep notices</Button>}
  </>}>
    <label className="grid gap-2 text-xs">Source and license notices<textarea aria-label="Source and license notices" className="min-h-64 w-full resize-y rounded border border-line bg-field p-2 font-mono text-xs" readOnly={readonly} disabled={busy} value={text} onChange={event => setText(event.target.value)} maxLength={2_000_000} /></label>
    <p className="mt-2 text-xs text-muted-foreground">These notices are saved with the native library and retained when its entries are copied to another library. Libraries with notices require a current PCBPlanner Studio version. Verify the source conditions before sharing a converted collection.</p>
    {error && <p role="alert" className="mt-2 text-xs text-destructive">{error}</p>}
  </Dialog>;
}
