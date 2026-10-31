import test from "node:test";
import assert from "node:assert/strict";
import { boardSurface, BoardSurface } from "../src/editor/boardSurface.ts";
import { inTriangles } from "../src/editor/shapes.ts";
import type { Item, Point } from "../src/core/types.ts";
const p=(x:number,y:number):Point=>({x:x*1e6,y:y*1e6});
const rect=(x:number,y:number,w:number,h:number):Item=>({kind:"rect",a:p(x,y),b:p(x+w,y+h),width:100_000,layer:14,fill:"none"});
const line=(a:Point,b:Point):Item=>({kind:"line",a,b,width:100_000,layer:14});
const has=(tris:number[],x:number,y:number)=>inTriangles(p(x,y),tris);

test("board surface distinguishes exterior, nested cutouts and separate islands",()=>{
 const triangles=boardSurface([rect(0,0,10,10),rect(2,2,6,6),rect(3,3,1,1),rect(12,0,2,2)]);
 assert.ok(has(triangles,1,1));assert.ok(!has(triangles,-1,1));assert.ok(!has(triangles,5,5));
 assert.ok(has(triangles,3.5,3.5));assert.ok(has(triangles,13,1));assert.ok(!has(triangles,11,1));
});
test("shuffled reversed outline edges close, while open chains remain unshaded",()=>{
 const edges=[line(p(0,0),p(10,0)),line(p(10,10),p(10,0)),line(p(0,10),p(10,10)),line(p(0,0),p(0,10))];
 assert.ok(has(boardSurface(edges),5,5));assert.deepEqual(boardSurface(edges.slice(1)),[]);
 assert.deepEqual(boardSurface([]),[]);
});
test("curved outlines and circular cutouts preserve material",()=>{
 const arc:Item={kind:"arc",center:p(0,0),radius:5e6,startAngle:0,sweep:180,width:100_000,layer:14};
 const hole:Item={kind:"circle",center:p(0,2),radius:1e6,width:100_000,layer:14,fill:"none"};
 const tris=boardSurface([arc,line(p(-5,0),p(5,0)),hole]);
 assert.ok(has(tris,2,2));assert.ok(!has(tris,0,2));assert.ok(!has(tris,0,-1));
 const poly:Item={kind:"polyline",points:[p(0,0),p(10,0),p(10,10),p(0,10)],closed:true,width:100_000,layer:14,fill:"none"};
 assert.ok(has(boardSurface([poly]),5,5));
});
test("copper changes reuse the material mesh; outline edits invalidate it",()=>{
 const cache=new BoardSurface(),outline=rect(0,0,10,10),items=[outline];
 const first=cache.read(items);assert.strictEqual(cache.read([...items,{...outline,layer:0}]),first);
 const moved=cache.read([rect(10,0,10,10)]);assert.notStrictEqual(moved,first);assert.ok(!has(moved,5,5));
 assert.deepEqual(cache.read([]),[]);
});