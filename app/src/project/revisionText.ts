import type {RevisionChanges,RevisionInfo} from '../core/api';
const description=(r:RevisionInfo)=>{const date=new Date(r.createdMs);return r.id+' · '+(r.label||'Checkpoint')+' · '+(Number.isFinite(date.getTime())?date.toISOString():'Unknown date');};
/** Exports exactly the reviewed comparison, including truncation notices. */
export function revisionChangesText(changes:RevisionChanges):string {
  const lines=['PCBPlanner Studio saved-version changelog','From: '+description(changes.from),'To: '+description(changes.to),'',changes.added+' files added · '+changes.removed+' removed · '+changes.modified+' modified',''];
  if(!changes.files.length)lines.push('No saved file changes between these checkpoints.');
  for(const file of changes.files){lines.push(file.status+' · '+file.path,file.summary,...file.details.map(detail=>'  - '+detail));if(file.omitted>0)lines.push('  '+file.omitted+' additional detail lines omitted from this comparison.');lines.push('');}
  lines.push('Saved snapshot comparison; unsaved edits are excluded.');return lines.join('\n')+'\n';
}
