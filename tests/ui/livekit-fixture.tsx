// Only resolved by the UI test server. It never opens a real room or microphone.
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

const Context = createContext<any>(null);

export function LiveKitRoom({ children }: { children: ReactNode }) {
  const [microphoneEnabled, setMicrophoneEnabled] = useState(true);
  const enabled = useRef(true);
  const room = useMemo(() => {
    const listeners = new Map<string, Set<(...args: any[]) => void>>();
    const room: any = {
      localParticipant: {
        get isMicrophoneEnabled() { return enabled.current; },
        async setMicrophoneEnabled(next: boolean) { enabled.current = next; setMicrophoneEnabled(next); },
        async publishData(data: Uint8Array, { topic }: { topic: string }) {
          if (topic === "archava.page") queueMicrotask(() => room.emit("dataReceived", data, { isAgent: true }, 0, "archava.page.ack"));
        },
      },
      on(event: string, listener: (...args: any[]) => void) {
        if (!listeners.has(event)) listeners.set(event, new Set());
        listeners.get(event)!.add(listener);
        return room;
      },
      off(event: string, listener: (...args: any[]) => void) { listeners.get(event)?.delete(listener); return room; },
      emit(event: string, ...args: any[]) { listeners.get(event)?.forEach(listener => listener(...args)); },
    };
    return room;
  }, []);
  useEffect(() => {
    (window as any).__ARCHAVA_UI_ROOM = room;
    return () => { delete (window as any).__ARCHAVA_UI_ROOM; };
  }, [room]);
  return <Context.Provider value={{ room, microphoneEnabled }}>{children}</Context.Provider>;
}
export function useRoomContext() { return useContext(Context).room; }
export function useLocalParticipant() { return { localParticipant: useContext(Context).room.localParticipant }; }
export function useConnectionState() { return "connected"; }
export function useVoiceAssistant() { return { state: "listening", audioTrack: undefined }; }
export function useTracks() { return []; }
export function isTrackReference() { return false; }
export function BarVisualizer() { return <div aria-hidden="true" />; }
export function RoomAudioRenderer() { return null; }
export function StartAudio() { return null; }
export function VideoTrack() { return null; }
export function DisconnectButton(props: any) { return <button type="button" {...props} />; }
