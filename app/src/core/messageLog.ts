import type {Message} from './store';
export interface MessageFilter {level:'all'|'error'|'warning';source:string;query:string}
export function filterMessages(messages:readonly Message[],filter:MessageFilter):Message[]{
  const query=filter.query.trim().toLowerCase();
  return messages.filter(message=>(filter.level==='all'||message.level===filter.level)&&(!filter.source||message.source===filter.source)&&(!query||[message.text,message.source??''].some(text=>text.toLowerCase().includes(query))));
}
export function messageLogText(messages:readonly Message[]):string{
  return messages.map(message=>message.time.toISOString()+' ['+message.level.toUpperCase()+']'+(message.source?' ['+message.source+']':'')+' '+message.text).join('\n');
}
