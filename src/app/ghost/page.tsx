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
    <>
      <style jsx global>{`
        html,
        body {
          background: transparent !important;
          overflow: hidden !important;
        }
      `}</style>
      <main className="pointer-events-none h-screen w-screen bg-transparent p-0">
        <div
          className={cn(
            "h-full w-full overflow-hidden whitespace-pre text-[13px] leading-6 text-zinc-400/85 transition-opacity",
            payload.visible ? "opacity-100" : "opacity-0",
          )}
        >
          {payload.text}
        </div>
      </main>
    </>
  );
}
