"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  acceptSuggestion,
  fetchAppState,
  listenErrors,
  listenState,
  updateAppSettings,
} from "@/lib/tauri-client";
import { AppSnapshot, defaultSnapshot, Settings } from "@/lib/typeace";
import { cn } from "@/lib/utils";

type UiLanguage = "zh" | "en";

const uiText = {
  zh: {
    title: "TypeAce",
    subtitle: "输入补全设置（本地 / 云端可切换）",
    uiLanguage: "界面语言",
    enabled: "已启用",
    disabled: "已禁用",
    enableTypeAce: "启用 TypeAce",
    triggerDelay: "触发延迟",
    minimumLength: "最小触发长度",
    writingStyle: "补全风格",
    styleCasual: "自然",
    styleProfessional: "专业",
    styleCreative: "创意",
    inferenceMode: "推理模式",
    inferenceLocal: "本地（Ollama）",
    inferenceCloud: "云端（GPU API）",
    completionLanguage: "补全语言",
    localSection: "本地模型",
    localBadge: "Local",
    cloudSection: "云端模型",
    cloudBadge: "Cloud",
    cloudBaseUrl: "云端接口地址",
    cloudApiKey: "云端 API Key",
    cloudModel: "云端模型名",
    cloudMaxTokens: "云端最大补全长度",
    localBaseUrl: "本地推理地址",
    localModelZh: "中文模型",
    localModelEn: "英文模型",
    localMaxTokens: "最大补全长度",
    boundaryOnlyTitle: "仅在空格/标点后请求",
    boundaryOnlyHint: "IME 输入场景更稳定",
    hotkey: "快捷键",
    custom: "自定义",
    customPlaceholder: "例如 Ctrl+Shift+Space",
    launchOnStartup: "开机自启",
    launchHint: "Windows 登录后后台运行",
    acceptedToday: "今日接受补全次数",
    acceptedValue: "次",
    testAccept: "测试接受补全",
    saving: "保存中...",
    loading: "加载中...",
    synced: "已同步",
    saveFailed: "设置保存失败",
    mvpPreset: "应用极速预设",
    mvpPresetHint: "最低延迟优先（可能降低准确率）",
  },
  en: {
    title: "TypeAce",
    subtitle: "Typing completion settings (local / cloud switchable)",
    uiLanguage: "UI language",
    enabled: "Enabled",
    disabled: "Disabled",
    enableTypeAce: "Enable TypeAce",
    triggerDelay: "Trigger delay",
    minimumLength: "Minimum trigger length",
    writingStyle: "Completion style",
    styleCasual: "Casual",
    styleProfessional: "Professional",
    styleCreative: "Creative",
    inferenceMode: "Inference mode",
    inferenceLocal: "Local (Ollama)",
    inferenceCloud: "Cloud (GPU API)",
    completionLanguage: "Completion language",
    localSection: "Local Models",
    localBadge: "Local",
    cloudSection: "Cloud Models",
    cloudBadge: "Cloud",
    cloudBaseUrl: "Cloud API URL",
    cloudApiKey: "Cloud API Key",
    cloudModel: "Cloud model",
    cloudMaxTokens: "Cloud max completion length",
    localBaseUrl: "Local inference URL",
    localModelZh: "Chinese model",
    localModelEn: "English model",
    localMaxTokens: "Max completion length",
    boundaryOnlyTitle: "Request only at space/punctuation",
    boundaryOnlyHint: "More stable for IME composition",
    hotkey: "Hotkey",
    custom: "Custom",
    customPlaceholder: "e.g. Ctrl+Shift+Space",
    launchOnStartup: "Launch on startup",
    launchHint: "Run in background after Windows login",
    acceptedToday: "Accepted completions today",
    acceptedValue: "times",
    testAccept: "Test accept completion",
    saving: "Saving...",
    loading: "Loading...",
    synced: "Synced",
    saveFailed: "Failed to save settings",
    mvpPreset: "Apply Turbo Preset",
    mvpPresetHint: "Lowest-latency priority (may reduce accuracy)",
  },
} as const;

const outputLanguageOptions = [
  { value: "auto", labelZh: "鑷姩锛堣窡闅忚緭鍏ワ級", labelEn: "Auto (follow input)" },
  { value: "zh", labelZh: "涓枃", labelEn: "Chinese" },
  { value: "en", labelZh: "鑻辨枃", labelEn: "English" },
  { value: "ja", labelZh: "鏃ヨ", labelEn: "Japanese" },
  { value: "ko", labelZh: "闊╄", labelEn: "Korean" },
  { value: "es", labelZh: "瑗胯", labelEn: "Spanish" },
  { value: "fr", labelZh: "娉曡", labelEn: "French" },
  { value: "de", labelZh: "寰疯", labelEn: "German" },
] as const;

const inferenceModeOptions = [
  { value: "local", labelKey: "inferenceLocal" },
  { value: "cloud", labelKey: "inferenceCloud" },
] as const;

function normalizeSettings(settings: Settings): Settings {
  const localBaseUrl = settings.localBaseUrl.trim();
  const localModelZh = settings.localModelZh.trim();
  const localModelEn = settings.localModelEn.trim();
  const localMaxTokens = Math.round(settings.localMaxTokens);
  const cloudBaseUrl = settings.cloudBaseUrl.trim();
  const cloudModel = settings.cloudModel.trim();
  const cloudMaxTokens = Math.round(settings.cloudMaxTokens);

  return {
    ...settings,
    inferenceMode: settings.inferenceMode === "cloud" ? "cloud" : "local",
    triggerDelayMs: Math.min(500, Math.max(60, Math.round(settings.triggerDelayMs))),
    minimumLength: Math.min(24, Math.max(1, Math.round(settings.minimumLength))),
    requestOnBoundaryOnly: false,
    customHotkey: settings.customHotkey.trim(),
    localBaseUrl: localBaseUrl || "http://127.0.0.1:11434/api/generate",
    localModelZh: localModelZh || "qwen2.5:1.5b",
    localModelEn: localModelEn || "llama3.2:1b",
    localMaxTokens: Math.min(48, Math.max(8, localMaxTokens)),
    cloudBaseUrl: cloudBaseUrl || "https://api.deepseek.com/v1/chat/completions",
    cloudApiKey: settings.cloudApiKey.trim(),
    cloudModel: cloudModel || "deepseek-chat",
    cloudMaxTokens: Math.min(48, Math.max(8, cloudMaxTokens)),
  };
}

export default function Home() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(defaultSnapshot);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [runtimeError, setRuntimeError] = useState("");
  const [uiLanguage, setUiLanguage] = useState<UiLanguage>("zh");

  const t = uiText[uiLanguage];

  const updateSettings = useCallback(
    async (nextSettings: Settings) => {
      const normalized = normalizeSettings(nextSettings);
      setSaving(true);
      try {
        const result = await updateAppSettings(normalized);
        setSnapshot(result);
        setRuntimeError("");
      } catch (error) {
        setRuntimeError(error instanceof Error ? error.message : t.saveFailed);
      } finally {
        setSaving(false);
      }
    },
    [t.saveFailed],
  );

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
    const saved = window.localStorage.getItem("typeace-ui-language");
    if (saved === "zh" || saved === "en") {
      setUiLanguage(saved);
    }
  }, []);

  useEffect(() => {
    window.localStorage.setItem("typeace-ui-language", uiLanguage);
  }, [uiLanguage]);

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

  const syncLabel = useMemo(() => {
    if (saving) {
      return t.saving;
    }
    if (loading) {
      return t.loading;
    }
    return t.synced;
  }, [loading, saving, t.loading, t.saving, t.synced]);

  const applyMvpPreset = useCallback(() => {
    patchSettings((prev) => ({
      ...prev,
      inferenceMode: "cloud",
      triggerDelayMs: 90,
      minimumLength: 1,
      aiStyle: "casual",
      requestOnBoundaryOnly: false,
      localBaseUrl: prev.localBaseUrl.trim() || "http://127.0.0.1:11434/api/generate",
      localModelZh: prev.localModelZh.trim() || "qwen2.5:1.5b",
      localModelEn: prev.localModelEn.trim() || "llama3.2:1b",
      localMaxTokens: 32,
    }));
  }, [patchSettings]);

  return (
    <main className="relative min-h-screen overflow-hidden bg-[radial-gradient(circle_at_15%_20%,#e2f4ff_0%,#f4f8ff_34%,#fdfdfd_100%)] p-5">
      <div className="mx-auto flex max-w-md flex-col gap-4">
        <Card className="border-zinc-200/70 bg-white/85 shadow-lg backdrop-blur">
          <CardHeader className="pb-3">
            <div className="flex items-start justify-between gap-3">
              <div>
                <CardTitle className="text-2xl font-semibold tracking-tight text-zinc-900">
                  {t.title}
                </CardTitle>
                <CardDescription>{t.subtitle}</CardDescription>
              </div>
              <div className="flex w-[170px] flex-col gap-1.5">
                <Label className="text-[10px] font-medium uppercase tracking-wide text-zinc-500">
                  {t.uiLanguage}
                </Label>
                <Select
                  value={uiLanguage}
                  onValueChange={(value) => setUiLanguage(value as UiLanguage)}
                >
                  <SelectTrigger className="h-8 text-xs">
                    <SelectValue placeholder={t.uiLanguage} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="zh">涓枃</SelectItem>
                    <SelectItem value="en">English</SelectItem>
                  </SelectContent>
                </Select>
                <Badge
                  variant={snapshot.settings.enabled ? "default" : "secondary"}
                  className={cn(
                    "justify-center",
                    snapshot.settings.enabled ? "bg-emerald-600 text-white" : "",
                  )}
                >
                  {snapshot.settings.enabled ? t.enabled : t.disabled}
                </Badge>
              </div>
            </div>
          </CardHeader>

          <CardContent className="space-y-4">
            <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
              <Label htmlFor="enable-toggle" className="text-sm font-medium text-zinc-800">
                {t.enableTypeAce}
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
                <Label className="text-sm font-medium text-zinc-800">{t.triggerDelay}</Label>
                <span className="text-xs font-medium text-zinc-500">
                  {snapshot.settings.triggerDelayMs}ms
                </span>
              </div>
              <Slider
                min={60}
                max={500}
                step={5}
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
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-zinc-800">{t.minimumLength}</Label>
                <span className="text-xs font-medium text-zinc-500">
                  {snapshot.settings.minimumLength}
                </span>
              </div>
              <Slider
                min={1}
                max={24}
                step={1}
                value={[snapshot.settings.minimumLength]}
                onValueChange={([value]) =>
                  patchSettings((prev) => ({
                    ...prev,
                    minimumLength: value,
                  }))
                }
              />
            </div>

            <div className="space-y-2 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">{t.writingStyle}</Label>
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
                  <SelectItem value="casual">{t.styleCasual}</SelectItem>
                  <SelectItem value="professional">{t.styleProfessional}</SelectItem>
                  <SelectItem value="creative">{t.styleCreative}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">{t.inferenceMode}</Label>
              <Select
                value={snapshot.settings.inferenceMode}
                onValueChange={(value) =>
                  patchSettings((prev) => ({
                    ...prev,
                    inferenceMode: value as Settings["inferenceMode"],
                  }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {inferenceModeOptions.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {t[item.labelKey as "inferenceLocal" | "inferenceCloud"]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">{t.completionLanguage}</Label>
              <Select
                value={snapshot.settings.outputLanguage}
                onValueChange={(value) =>
                  patchSettings((prev) => ({
                    ...prev,
                    outputLanguage: value as Settings["outputLanguage"],
                  }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {outputLanguageOptions.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {uiLanguage === "zh" ? item.labelZh : item.labelEn}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-3 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-zinc-800">{t.localSection}</Label>
                <Badge variant="outline" className="text-[10px]">
                  {t.localBadge}
                </Badge>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
                <p className="text-[11px] text-zinc-600">{t.mvpPresetHint}</p>
                <Button variant="secondary" size="sm" onClick={applyMvpPreset}>
                  {t.mvpPreset}
                </Button>
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.localBaseUrl}</Label>
                <Input
                  value={snapshot.settings.localBaseUrl}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      localBaseUrl: event.target.value,
                    }))
                  }
                  placeholder="http://127.0.0.1:11434/api/generate"
                />
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.localModelZh}</Label>
                <Input
                  value={snapshot.settings.localModelZh}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      localModelZh: event.target.value,
                    }))
                  }
                  placeholder="qwen2.5:1.5b"
                />
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.localModelEn}</Label>
                <Input
                  value={snapshot.settings.localModelEn}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      localModelEn: event.target.value,
                    }))
                  }
                  placeholder="llama3.2:1b"
                />
              </div>

              <div className="space-y-2 rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
                <div className="flex items-center justify-between">
                  <Label className="text-xs text-zinc-600">{t.localMaxTokens}</Label>
                  <span className="text-xs font-medium text-zinc-500">
                    {snapshot.settings.localMaxTokens}
                  </span>
                </div>
                <Slider
                  min={8}
                  max={48}
                  step={4}
                  value={[snapshot.settings.localMaxTokens]}
                  onValueChange={([value]) =>
                    patchSettings((prev) => ({
                      ...prev,
                      localMaxTokens: value,
                    }))
                  }
                />
              </div>

              <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
                <div>
                  <Label className="text-xs font-medium text-zinc-700">{t.boundaryOnlyTitle}</Label>
                  <p className="text-[11px] text-zinc-500">{t.boundaryOnlyHint}</p>
                </div>
                <Switch
                  checked={snapshot.settings.requestOnBoundaryOnly}
                  onCheckedChange={(checked) =>
                    patchSettings((prev) => ({
                      ...prev,
                      requestOnBoundaryOnly: checked,
                    }))
                  }
                />
              </div>

            </div>

            <div className="space-y-3 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-zinc-800">{t.cloudSection}</Label>
                <Badge variant="outline" className="text-[10px]">
                  {t.cloudBadge}
                </Badge>
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.cloudBaseUrl}</Label>
                <Input
                  value={snapshot.settings.cloudBaseUrl}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      cloudBaseUrl: event.target.value,
                    }))
                  }
                  placeholder="https://api.deepseek.com/v1/chat/completions"
                />
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.cloudApiKey}</Label>
                <Input
                  type="password"
                  value={snapshot.settings.cloudApiKey}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      cloudApiKey: event.target.value,
                    }))
                  }
                  placeholder="sk-..."
                />
              </div>

              <div className="grid gap-1.5">
                <Label className="text-xs text-zinc-600">{t.cloudModel}</Label>
                <Input
                  value={snapshot.settings.cloudModel}
                  onChange={(event) =>
                    patchSettings((prev) => ({
                      ...prev,
                      cloudModel: event.target.value,
                    }))
                  }
                  placeholder="deepseek-chat"
                />
              </div>

              <div className="space-y-2 rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
                <div className="flex items-center justify-between">
                  <Label className="text-xs text-zinc-600">{t.cloudMaxTokens}</Label>
                  <span className="text-xs font-medium text-zinc-500">
                    {snapshot.settings.cloudMaxTokens}
                  </span>
                </div>
                <Slider
                  min={8}
                  max={48}
                  step={4}
                  value={[snapshot.settings.cloudMaxTokens]}
                  onValueChange={([value]) =>
                    patchSettings((prev) => ({
                      ...prev,
                      cloudMaxTokens: value,
                    }))
                  }
                />
              </div>
            </div>

            <div className="space-y-3 rounded-lg border border-zinc-200 bg-white px-3 py-3">
              <Label className="text-sm font-medium text-zinc-800">{t.hotkey}</Label>
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
                  <Label htmlFor="hotkey-custom">{t.custom}</Label>
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
                  placeholder={t.customPlaceholder}
                />
              ) : null}
            </div>

            <div className="flex items-center justify-between rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2">
              <div>
                <Label htmlFor="autostart-toggle" className="text-sm font-medium text-zinc-800">
                  {t.launchOnStartup}
                </Label>
                <p className="text-xs text-zinc-500">{t.launchHint}</p>
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
            <CardTitle className="text-base font-semibold">{t.acceptedToday}</CardTitle>
            <CardDescription>
              {snapshot.usage.usedToday} {t.acceptedValue}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <Button
              variant="secondary"
              disabled={!snapshot.settings.enabled}
              onClick={async () => {
                await acceptSuggestion();
              }}
            >
              {t.testAccept}
            </Button>
            <div className="text-xs text-zinc-500">{syncLabel}</div>
            {runtimeError ? (
              <p className="rounded-md bg-red-50 px-2 py-1 text-xs text-red-600">{runtimeError}</p>
            ) : null}
          </CardContent>
        </Card>
      </div>
    </main>
  );
}

