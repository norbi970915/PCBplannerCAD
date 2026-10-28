import { useEffect, useMemo, useRef, useState } from "react";
import { CircleAlert, Info, ShieldCheck, Trash2, TriangleAlert } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../core/api";
import { filterMessages, messageLogText } from "../core/messageLog";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { compileProject, navigateTo, report } from "../core/actions";
import { useEditor, type Message } from "../core/store";

type Filter = "all" | "error" | "warning";

const LEVEL_ICON: Record<Message["level"], React.ReactNode> = {
  error: <CircleAlert className="size-3.5 text-err-line" />,
  warning: <TriangleAlert className="size-3.5 text-note-line" />,
  info: <Info className="size-3.5 text-faint" />,
};

export function MessagesPanel() {
  const messages = useEditor((s) => s.messages);
  const hasProject = useEditor((s) => !!s.project);
  const [filter, setFilter] = useState<Filter>("all");
  const [query,setQuery]=useState(""),[source,setSource]=useState(""),[follow,setFollow]=useState(true),[saving,setSaving]=useState(false);
  const sources=useMemo(()=>[...new Set(messages.flatMap(m=>m.source?[m.source]:[]))].sort(),[messages]);
  const logRef = useRef<HTMLDivElement>(null);
  const shown = useMemo(()=>filterMessages(messages,{level:filter,source,query}),[messages,filter,source,query]);
  const exportLog=async()=>{setSaving(true);try{const path=await save({defaultPath:"pCAD-log.txt",filters:[{name:"Text log",extensions:["txt"]}]});if(path)await api.writeFileBytes(path,new TextEncoder().encode(messageLogText(shown)));}catch(e){report(e);}finally{setSaving(false);}};
  const errors = messages.filter((m) => m.level === "error").length;
  const warnings = messages.filter((m) => m.level === "warning").length;

  useEffect(() => {
    // Scroll only the log: scrollIntoView can also scroll dock panels and hide their headers.
    if (follow && logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [messages,follow,filter,source,query]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-panel text-xs">
      <div className="flex h-8 shrink-0 items-center gap-2 overflow-x-auto whitespace-nowrap border-b border-line bg-panel-head px-2">
        <ToggleGroup type="single" value={filter} onValueChange={(v) => v && setFilter(v as Filter)} className="h-6">
          <ToggleGroupItem value="all" className="h-6 px-2 text-[11px]">
            All <span className="font-mono text-faint">{messages.length}</span>
          </ToggleGroupItem>
          <ToggleGroupItem value="error" className="h-6 px-2 text-[11px]">
            Errors <span className="font-mono text-faint">{errors}</span>
          </ToggleGroupItem>
          <ToggleGroupItem value="warning" className="h-6 px-2 text-[11px]">
            Warnings <span className="font-mono text-faint">{warnings}</span>
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="flex-1" />
        {hasProject && (
          <Button variant="ghost" size="sm" onClick={() => void compileProject()}>
            <ShieldCheck /> Run ERC
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={() => useEditor.getState().clearMessages()}>
          <Trash2 /> Clear
        </Button>
      </div>
      <div className="flex shrink-0 items-center gap-2 overflow-x-auto whitespace-nowrap border-b border-line px-2 py-1 [&>button]:shrink-0">
        <Input className="h-7 w-48 min-w-32 flex-1" aria-label="Search messages" placeholder="Search messages…" value={query} onChange={e=>setQuery(e.target.value)}/>
        <NativeSelect wrapperClassName="w-36 shrink-0" aria-label="Message source" value={source} onChange={e=>setSource(e.target.value)}><NativeSelectOption value="">All sources</NativeSelectOption>{[...new Set([...sources,...(source?[source]:[])])].map(name=><NativeSelectOption key={name} value={name}>{name}</NativeSelectOption>)}</NativeSelect>
        <button className={'rounded px-2 py-1 text-[11px] hover:bg-hover '+(follow?'text-accent-ink':'text-faint')} aria-pressed={follow} title="Follow new messages; scrolling up pauses following" onClick={()=>setFollow(!follow)}>Follow</button>
        <Button variant="ghost" size="sm" disabled={!shown.length} onClick={()=>void navigator.clipboard.writeText(messageLogText(shown)).catch(report)}>Copy shown</Button>
        <Button variant="ghost" size="sm" disabled={!shown.length||saving} onClick={()=>void exportLog()}>Save log</Button>
        <span className="text-[10px] text-faint">{shown.length}/{messages.length}</span>
      </div>
      <div ref={logRef} className="min-h-0 flex-1 overflow-auto overscroll-contain" onScroll={e=>{const element=e.currentTarget;setFollow(element.scrollHeight-element.scrollTop-element.clientHeight<24);}}>
        <div className="sticky top-0 grid grid-cols-[24px_74px_64px_1fr] border-b border-line bg-panel-head px-2 py-1 text-[10.5px] font-semibold text-faint">
          <span />
          <span>Time</span>
          <span>Source</span>
          <span>Message</span>
        </div>
        {shown.map((m) => (
          <div
            key={m.id}
            onClick={() => m.target && void navigateTo(m.target)}
            title={m.target ? "Click to show in the editor" : undefined}
            className={cn(
              "grid grid-cols-[24px_74px_64px_1fr] items-start border-b border-line/60 px-2 py-[3px]",
              m.target && "cursor-pointer hover:bg-hover",
              m.level === "error" && "bg-err/40",
            )}
          >
            <span className="pt-px">{LEVEL_ICON[m.level]}</span>
            <span className="font-mono text-[11px] text-faint">{m.time.toLocaleTimeString()}</span>
            <span className="text-faint">{m.source ?? ""}</span>
            <span className="min-w-0 break-words select-text">{m.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
