"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  acceptSuggestion,
  clearUsage,
  fetchAppState,
  listenErrors,
  listenState,
  updateAppSettings,
} from "@/lib/tauri-client";
import { AppSnapshot, defaultSnapshot, Settings } from "@/lib/typeace";
import { cn } from "@/lib/utils";

function normalizeSettings(settings: Settings): Settings {
  return {
    ...settings,
    triggerDelayMs: Math.min(800, Math.max(200, Math.round(settings.triggerDelayMs))),
    minimumLength: Math.max(10, Math.round(settings.minimumLength)),
  };
}

function usagePercentage(snapshot: AppSnapshot): number {
  if (snapshot.settings.isPro) {
    return 0;
  }
  return Math.min(100, (snapshot.usage.usedToday / snapshot.usage.freeLimit) * 100);
}

export default function Home() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(defaultSnapshot);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [runtimeError, setRuntimeError] = useState<string>("");

  const remaining = snapshot.remainingToday ?? Number.POSITIVE_INFINITY;
  const canUseFree = snapshot.settings.isPro || remaining > 0;

  const updateSettings = useCallback(async (nextSettings: Settings) => {
    const normalized = normalizeSettings(nextSettings);
    setSaving(true);
    try {
      const result = await updateAppSettings(normalized);
      setSnapshot(result);
      setRuntimeError("");
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : "更新设置失败");
    } finally {
      setSaving(false);
    }
  }, []);

  const patchSettings = useCallback(
    (mutate: (prev: Settings) => Settings) => {
      setSnapshot((prev) => {
        const next = normalizeSettings(mutate(prev.settings));
        void updateSettings(next);
        return {
          ...prev,
          settings: next,
        };
      });
    },
    [updateSettings],
  );

  useEffect(() => {
    let alive = true;
    let unlistenState: () => void = () => {};
    let unlistenErrors: () => void = () => {};

    (async () => {
      try {
        const data = await fetchAppState();
        if (alive) {
          setSnapshot(data);
        }
      } finally {
        if (alive) {
          setLoading(false);
        }
      }

      unlistenState = await listenState((payload) => {
        if (alive) {
          setSnapshot(payload);
        }
      });

      unlistenErrors = await listenErrors((message) => {
        if (alive) {
          setRuntimeError(message);
        }
      });
    })();

    return () => {
      alive = false;
      unlistenState();
      unlistenErrors();
    };
  }, []);

  const usageLabel = useMemo(() => {
    if (snapshot.settings.isPro) {
      return "无限次 / Pro";
    }
    return `${snapshot.usage.usedToday} / ${snapshot.usage.freeLimit} 次`;
  }, [snapshot]);

  return (
    <main className="relative min-h-screen overflow-hidden bg-[radial-gradient(circle_at_15%_20%,#e2f4ff_0%,#f4f8ff_34%,#fdfdfd_100%)] p-5">
      <div className="mx-auto flex max-w-md flex-col gap-4">
        <Card className="border-zinc-200/70 bg-white/85 shadow-lg backdrop-blur">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-2xl font-semibold tracking-tight text-zinc-900">
                TypeAce
              </CardTitle>
              <Badge
                variant={snapshot.settings.enabled ? "default" : "secondary"}
                className={cn(snapshot.settings.enabled ? "bg-emerald-600 text-white" : "")}
              >
                {snapshot.settings.enabled ? "Enabled" : "Disabled"}
              </Badge>
            </div>
            <CardDescription>全局 AI 打字补全设置</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
              <Label htmlFor="enable-toggle" className="text-sm font-medium text-zinc-800">
                总开关
              </Label>
              <Switch
                id="enable-toggle"
                checked={snapshot.settings.enabled}
                onCheckedChange={(checked) =>
                  patchSettings((prev) => ({
                    ...prev,
                    enabled: checked,
                  }))
                }
              />
            </div>

            <div className="space-y-2 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-zinc-800">触发延迟</Label>
                <span className="text-xs font-medium text-zinc-500">
                  {snapshot.settings.triggerDelayMs}ms
                </span>
              </div>
              <Slider
                min={200}
                max={800}
                step={10}
                value={[snapshot.settings.triggerDelayMs]}
                onValueChange={([value]) =>
                  patchSettings((prev) => ({
                    ...prev,
                    triggerDelayMs: value,
                  }))
                }
              />
            </div>

            <div className="space-y-2 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">AI 风格</Label>
              <Select
                value={snapshot.settings.aiStyle}
                onValueChange={(value) =>
                  patchSettings((prev) => ({
                    ...prev,
                    aiStyle: value as Settings["aiStyle"],
                  }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="casual">Casual</SelectItem>
                  <SelectItem value="professional">Professional</SelectItem>
                  <SelectItem value="creative">Creative</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-3 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">快捷键设置</Label>
              <RadioGroup
                value={snapshot.settings.hotkeyMode}
                onValueChange={(value) =>
                  patchSettings((prev) => ({
                    ...prev,
                    hotkeyMode: value as Settings["hotkeyMode"],
                  }))
                }
                className="grid gap-2"
              >
                <div className="flex items-center gap-2 rounded-md border border-zinc-200 px-2 py-1.5">
                  <RadioGroupItem value="tab" id="hotkey-tab" />
                  <Label htmlFor="hotkey-tab">Tab</Label>
                </div>
                <div className="flex items-center gap-2 rounded-md border border-zinc-200 px-2 py-1.5">
                  <RadioGroupItem value="ctrlSpace" id="hotkey-ctrl-space" />
                  <Label htmlFor="hotkey-ctrl-space">Ctrl+Space</Label>
                </div>
                <div className="flex items-center gap-2 rounded-md border border-zinc-200 px-2 py-1.5">
                  <RadioGroupItem value="custom" id="hotkey-custom" />
                  <Label htmlFor="hotkey-custom">自定义</Label>
                </div>
              </RadioGroup>
              {snapshot.settings.hotkeyMode === "custom" ? (
                <Input
                  value={snapshot.settings.customHotkey}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      customHotkey: event.target.value,
                    }))
                  }
                  placeholder="例如 Ctrl+Shift+Space"
                />
              ) : null}
            </div>

            <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
              <div>
                <Label htmlFor="autostart-toggle" className="text-sm font-medium text-zinc-800">
                  开机自启
                </Label>
                <p className="text-xs text-zinc-500">Windows 启动后后台运行</p>
              </div>
              <Switch
                id="autostart-toggle"
                checked={snapshot.settings.autostart}
                onCheckedChange={(checked) =>
                  patchSettings((prev) => ({
                    ...prev,
                    autostart: checked,
                  }))
                }
              />
            </div>
          </CardContent>
        </Card>

        <Card className="border-zinc-200/70 bg-white/90 shadow-md">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base font-semibold">套餐状态</CardTitle>
              <Badge variant={snapshot.settings.isPro ? "default" : "secondary"}>
                {snapshot.settings.isPro ? "Pro" : "Free"}
              </Badge>
            </div>
            <CardDescription>
              {snapshot.settings.isPro ? "高级模型 + 无限补全" : "基础模型 + 每日 50 次"}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs font-medium text-zinc-600">今日使用量</span>
                <span className="text-xs font-semibold text-zinc-900">{usageLabel}</span>
              </div>
              <Progress value={usagePercentage(snapshot)} />
              <div className="mt-2 text-xs text-zinc-500">
                剩余：{snapshot.settings.isPro ? "无限" : `${remaining} 次`}
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <Button
                variant="outline"
                onClick={() =>
                  patchSettings((prev) => ({
                    ...prev,
                    isPro: !prev.isPro,
                  }))
                }
              >
                {snapshot.settings.isPro ? "切回 Free" : "升级 Pro"}
              </Button>
              <Button
                variant="outline"
                onClick={async () => {
                  const reset = await clearUsage();
                  setSnapshot(reset);
                }}
              >
                重置计数
              </Button>
            </div>

            <Separator />

            <div className="grid grid-cols-2 gap-2 text-xs">
              <Button
                variant="secondary"
                disabled={!canUseFree || !snapshot.settings.enabled}
                onClick={async () => {
                  await acceptSuggestion();
                }}
              >
                测试 Tab 补全
              </Button>
              <div className="flex items-center justify-end text-zinc-500">
                {saving ? "保存中..." : loading ? "加载中..." : "已同步"}
              </div>
            </div>

            {runtimeError ? (
              <p className="rounded-md bg-red-50 px-2 py-1 text-xs text-red-600">{runtimeError}</p>
            ) : null}
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
