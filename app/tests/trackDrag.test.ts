import test from 'node:test';
import assert from 'node:assert/strict';
const mm=(v:number)=>Math.round(v*1e6);
const p=(x:number,y:number)=>({x:mm(x),y:mm(y)});

test('direction rails hold horizontal and diagonal runs through pointer noise, then release',async()=>{
 const {snapRouteDirection}=await import('../src/editor/routeDirection.ts');
 const a=snapRouteDirection(p(0,0),p(5,.03),mm(.05));assert.equal(a.axis,0);assert.deepEqual(a.point,p(5,0));
 const held=snapRouteDirection(p(0,0),p(5,.07),mm(.05),a.axis);assert.equal(held.axis,0);
 const released=snapRouteDirection(p(0,0),p(5,.2),mm(.05),held.axis);assert.equal(released.axis,null);assert.deepEqual(released.point,p(5,.2));
 const diagonal=snapRouteDirection(p(0,0),p(5,5.03),mm(.05));assert.equal(diagonal.axis,1);assert.equal(diagonal.point.x,diagonal.point.y);
 const reverse=snapRouteDirection(p(0,0),p(-5,0),mm(.05),0);assert.equal(reverse.axis,4);assert.deepEqual(reverse.point,p(-5,0));
 });
