import {createContext,useContext} from 'react';
export const SurfaceWindowContext=createContext<Window|null>(null);
export function useSurfaceWindow(){return useContext(SurfaceWindowContext)??window;}
