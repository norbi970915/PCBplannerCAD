import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Dialog, useModal } from "../shell/modal";
import { api } from "../core/api";
import terms from "../../../LICENSE.md?raw";
import privacy from "../../../LICENSE/PRIVACY.md?raw";
import sources from "../../../LICENSE/SOURCE-AVAILABILITY.md?raw";
import notices from "../../../LICENSE/generated/third-party-notices.txt?raw";

const documents = { about: "", terms, privacy, notices, sources };
type Section = keyof typeof documents;
const labels: Record<Section, string> = { about: "About", terms: "Free-use terms", privacy: "Local data", notices: "Third-party licenses", sources: "Source availability" };

export function LegalDialog() {
  const [section, setSection] = useState<Section>("about");
  const [folders, setFolders] = useState<{ legal: string; data: string; migration: string[] } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => { api.legalInfo().then(setFolders).catch(reason => setError(String(reason))); }, []);
  return <Dialog title="About PCBPlanner Studio" description="Personal project by Buchmüller Norbert · pcbplanner.info@gmail.com" wide footer={<>
    <Button variant="outline" onClick={() => api.openLegalFolder().catch(reason => setError(String(reason)))}>Open legal files</Button>
    <Button onClick={() => useModal.getState().close()}>Close</Button>
  </>}>
    <Tabs value={section} onValueChange={value => setSection(value as Section)}>
      <TabsList className="mb-3 flex h-auto flex-wrap justify-start">{Object.entries(labels).map(([key, label]) => <TabsTrigger key={key} value={key}>{label}</TabsTrigger>)}</TabsList>
    </Tabs>
    {section === "about" ? <div className="space-y-3 text-xs leading-relaxed">
      <p><strong>PCBPlanner Studio 0.6.6</strong> — schematic capture, PCB layout, manufacturing outputs, 3D view and external design import.</p>
      <p>Copyright © 2026 Buchmüller Norbert. PCBPlanner Studio is a personal project, free to use for personal, educational and professional work under the included terms.</p>
      <p>Design-rule results, footprint defaults and calculations are engineering aids. Verify device drawings and fabrication outputs before manufacture. No standards certification is claimed.</p>
      <p>VRML model previews are supported. STEP model previews and thumbnails are temporarily unavailable; model references are preserved and STEP export remains available.</p>
      <p>Third-party software retains its licenses. Required notices and covered source archives accompany this application.</p>
      {folders && <div className="space-y-1 break-all"><p><strong>Local data:</strong> {folders.data}</p><p><strong>Legal files:</strong> {folders.legal}</p>{folders.migration.map((message, i) => <p key={i} role="status">{message}</p>)}</div>}
    </div> : <pre className="whitespace-pre-wrap break-words font-sans text-xs leading-relaxed" tabIndex={0} aria-label={labels[section]}>{documents[section]}</pre>}
    {error && <p className="mt-3 text-xs text-destructive" role="alert">{error}</p>}
  </Dialog>;
}

export function openLegalDialog() { useModal.getState().show(<LegalDialog />); }
