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
          margin: 0 !important;
          padding: 0 !important;
        }
      `}</style>
      <main className="pointer-events-none h-screen w-screen bg-transparent p-0">
        <div
          className={cn(
            "h-full w-full whitespace-pre pt-[3px] text-[14px] leading-[1.45] text-zinc-600/80 [text-shadow:0_1px_0_rgba(255,255,255,0.65)] transition-opacity",
            payload.visible ? "opacity-100" : "opacity-0",
          )}
          style={{
            fontFamily:
              "Segoe UI, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Arial, sans-serif",
          }}
        >
          {payload.text}
        </div>
      </main>
    </>
  );
}
