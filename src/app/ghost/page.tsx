"use client";

import { useEffect, useState } from "react";

import { listenGhostPayload } from "@/lib/tauri-client";
import { GhostEventPayload } from "@/lib/typeace";
import { cn } from "@/lib/utils";

const initialGhost: GhostEventPayload = {
  text: "",
  visible: false,
};

export default function GhostPage() {
  const [payload, setPayload] = useState<GhostEventPayload>(initialGhost);

  useEffect(() => {
    let unlisten: () => void = () => {};

    (async () => {
      unlisten = await listenGhostPayload((next) => {
        setPayload(next);
      });
    })();

    return () => {
      unlisten();
    };
  }, []);

  return (
    <main className="pointer-events-none h-screen w-screen bg-transparent p-0">
      <div
        className={cn(
          "h-full w-full rounded-md border border-zinc-300/50 bg-white/20 px-2 py-1 text-sm text-zinc-500 backdrop-blur-sm transition-opacity",
          payload.visible ? "opacity-100" : "opacity-0",
        )}
      >
        {payload.text}
      </div>
    </main>
  );
}
