import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { useToastStore } from "../stores/toastStore";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Switch } from "../components/ui/switch";
import { FolderOpen } from "lucide-react";


export default function Settings() {
    const {
        config,
        resources,
        fetchInitialData,
        selectWorkDirectory: selectWorkDirAction,
        togglePolling: togglePollingAction,
        setPollingInterval,
        setRetentionDays,
        updateConfig
    } = useAppStore();

    const { addToast } = useToastStore();

    // Local state for interval to manage input changes before committing
    const [localInterval, setLocalInterval] = useState(60);
    const [localRetention, setLocalRetention] = useState<number | null>(null);
    const [availableCategories, setAvailableCategories] = useState<string[]>([]);
    const [localAutoDownloadCats, setLocalAutoDownloadCats] = useState<string[]>([]);
    const [localDownloadMode, setLocalDownloadMode] = useState<'Queue' | 'Parallel'>('Queue');
    const [localPreferOptimized, setLocalPreferOptimized] = useState(true);

    useEffect(() => {
        fetchInitialData();
    }, [fetchInitialData]);

    useEffect(() => {
        if (config) {
            setLocalInterval(config.polling_interval_minutes);
            setLocalRetention(config.retention_days);
            setLocalAutoDownloadCats(config.auto_download_categories);
            setLocalDownloadMode(config.download_mode);
            setLocalPreferOptimized(config.prefer_optimized);
        }
    }, [config]);

    // Derive available categories from resources and config
    useEffect(() => {
        const cats = new Set<string>();
        // Add from current resources
        resources.forEach(r => cats.add(r.category));
        // Add from config (persisted ones even if no current resources)
        if (config) {
            config.auto_download_categories.forEach(c => cats.add(c));
        }
        setAvailableCategories(Array.from(cats).sort());
    }, [resources, config]);

    const toggleCategory = async (category: string, checked: boolean) => {
        if (!config) return;

        let newCats = [...localAutoDownloadCats];
        if (checked) {
            if (!newCats.includes(category)) newCats.push(category);
        } else {
            newCats = newCats.filter(c => c !== category);
        }

        setLocalAutoDownloadCats(newCats);

        try {
            await updateConfig({ auto_download_categories: newCats });
            addToast(`Auto-download ${checked ? 'enabled' : 'disabled'} for "${category}"`, "success");
        } catch (e) {
            addToast(`Failed to update category: ${e}`, "error");
            // Revert to config state on error
            if (config) setLocalAutoDownloadCats(config.auto_download_categories);
        }
    };

    const updateDownloadMode = async (mode: 'Queue' | 'Parallel') => {
        if (!config || mode === config.download_mode) return;
        setLocalDownloadMode(mode);
        try {
            await updateConfig({ download_mode: mode });
            addToast(`Download mode set to ${mode}`, "success");
        } catch (e) {
            addToast(`Failed to update mode: ${e}`, "error");
            if (config) setLocalDownloadMode(config.download_mode);
        }
    };

    const togglePreferOptimized = async (checked: boolean) => {
        if (!config || checked === config.prefer_optimized) return;
        setLocalPreferOptimized(checked);
        try {
            await updateConfig({ prefer_optimized: checked });
            addToast(checked ? "Video ottimizzati preferiti" : "Video originali preferiti", "success");
        } catch (e) {
            addToast(`Failed to update preference: ${e}`, "error");
            if (config) setLocalPreferOptimized(config.prefer_optimized);
        }
    };

    const handleIntervalBlur = async () => {
        if (!config) return;
        if (localInterval === config.polling_interval_minutes) return;

        if (isNaN(localInterval) || localInterval < 1 || localInterval > 1440) {
            addToast("Polling interval must be between 1 and 1440 minutes", "error");
            // Reset to last valid config value
            setLocalInterval(config.polling_interval_minutes);
            return;
        }

        try {
            await setPollingInterval(localInterval);
            addToast("Polling interval updated", "success");
        } catch (e) {
            addToast(`Failed to update interval: ${e}`, "error");
        }
    };

    const handleRetentionBlur = async () => {
        if (!config) return;
        if (localRetention === config.retention_days) return;

        if (localRetention !== null && localRetention < 0) {
            addToast("Retention days cannot be negative", "error");
            setLocalRetention(config.retention_days);
            return;
        }

        try {
            await setRetentionDays(localRetention);
            addToast("Retention policy updated", "success");
        } catch (e) {
            addToast(`Failed to update retention: ${e}`, "error");
        }
    };

    const togglePolling = async (enabled: boolean) => {
        try {
            await togglePollingAction(enabled);
            addToast(enabled ? "Polling enabled" : "Polling paused", "success");
        } catch (e) {
            addToast(`Failed to toggle polling: ${e}`, "error");
        }
    };

    const selectWorkDirectory = async () => {
        try {
            // const oldPath = config?.work_directory;
            await selectWorkDirAction();
            // check store to see if it changed (the action in store updates the state)
            // But selectWorkDirAction is async, so we should wait or check return if it returned something
            // currently selectWorkDirectory in appStore doesn't return anything but updates state.
            addToast("Work directory updated", "success");
        } catch (e) {
            addToast(`Failed to select directory: ${e}`, "error");
        }
    }

    if (!config) return <div>Loading settings...</div>;

    return (
        <div className="space-y-6 max-w-4xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Settings</h2>
                <p className="text-muted-foreground mt-1">
                    Manage how the application downloads and manages files.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>Storage</CardTitle>
                    <CardDescription>
                        Where files will be downloaded and stored.
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="flex flex-col gap-2">
                        <label className="text-sm font-medium">Work Directory</label>
                        <div className="flex gap-2">
                            <Input
                                readOnly
                                value={config.work_directory || "Not configured"}
                                className={!config.work_directory ? "text-muted-foreground italic" : ""}
                            />
                            <Button variant="outline" onClick={selectWorkDirectory}>
                                <FolderOpen className="mr-2 h-4 w-4" />
                                Select
                            </Button>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            All downloaded resources and archives will be stored here.
                        </p>
                    </div>

                    <div className="flex flex-col gap-2 pt-4 border-t">
                        <label className="text-sm font-medium">Retention Policy</label>
                        <div className="flex flex-wrap items-center gap-4">
                            <div className="flex items-center gap-2 w-48 min-w-[140px]">
                                <Input
                                    type="number"
                                    min="0"
                                    placeholder="Days"
                                    className="flex-1 min-w-0"
                                    value={localRetention === null ? "" : localRetention}
                                    onChange={(e) => {
                                        const val = e.target.value;
                                        setLocalRetention(val === "" ? null : parseInt(val));
                                    }}
                                    onBlur={handleRetentionBlur}
                                />
                                <span className="text-sm shrink-0">days</span>
                            </div>
                            <span className="text-sm text-muted-foreground">
                                (0 = delete immediately, empty = keep forever)
                            </span>
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Auto-Download</CardTitle>
                    <CardDescription>
                        Automatically download new resources for these categories.
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    {availableCategories.length === 0 ? (
                        <div className="text-sm text-muted-foreground italic">
                            No categories discovered yet. Visit the Dashboard to load resources.
                        </div>
                    ) : (
                        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
                            {availableCategories.map(cat => (
                                <div key={cat} className="flex items-center justify-between space-x-4 border p-3 rounded-lg bg-card/50">
                                    <label htmlFor={`switch-${cat}`} className="text-sm font-medium capitalize cursor-pointer flex-1">
                                        {cat}
                                    </label>
                                    <Switch
                                        id={`switch-${cat}`}
                                        checked={localAutoDownloadCats.includes(cat)}
                                        onCheckedChange={(checked) => toggleCategory(cat, checked)}
                                    />
                                </div>
                            ))}
                        </div>
                    )}
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Automation</CardTitle>
                    <CardDescription>
                        Configure automatic background checking for new resources.
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-6">
                    <div className="flex items-center justify-between gap-4">
                        <div className="space-y-0.5">
                            <label className="text-base font-medium">Enable Background Polling</label>
                            <p className="text-sm text-muted-foreground">
                                Automatically check for new content periodically.
                            </p>
                        </div>
                        <Switch
                            checked={config.polling_enabled}
                            onCheckedChange={togglePolling}
                        />
                    </div>

                    <div className="flex flex-col gap-2 pt-4 border-t">
                        <label className="text-sm font-medium">Download Strategy</label>
                        <div className="flex gap-6">
                            <div className="flex items-center space-x-2">
                                <input
                                    type="radio"
                                    id="mode-queue"
                                    name="download-mode"
                                    checked={localDownloadMode === 'Queue'}
                                    onChange={() => updateDownloadMode('Queue')}
                                    className="accent-primary h-4 w-4"
                                />
                                <label htmlFor="mode-queue" className="text-sm cursor-pointer select-none">Queue (Sequential)</label>
                            </div>
                            <div className="flex items-center space-x-2">
                                <input
                                    type="radio"
                                    id="mode-parallel"
                                    name="download-mode"
                                    checked={localDownloadMode === 'Parallel'}
                                    onChange={() => updateDownloadMode('Parallel')}
                                    className="accent-primary h-4 w-4"
                                />
                                <label htmlFor="mode-parallel" className="text-sm cursor-pointer select-none">Parallel (4x)</label>
                            </div>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            Queue downloads one file at a time. Parallel downloads up to 4 files simultaneously.
                        </p>
                    </div>

                    <div className="flex items-center justify-between gap-4 pt-4 border-t">
                        <div className="space-y-0.5">
                            <label className="text-base font-medium">Preferisci Video Ottimizzati</label>
                            <p className="text-sm text-muted-foreground">
                                Il file ottimizzato ha la stessa qualità percepita ma pesa fino a 10 volte di meno. Ogni risorsa ottimizzata è fornita grazie al lavoro dei volontari.
                            </p>
                        </div>
                        <Switch
                            checked={localPreferOptimized}
                            onCheckedChange={togglePreferOptimized}
                        />
                    </div>

                    <div className="flex flex-col gap-2 pt-4 border-t">
                        <label className="text-sm font-medium">Polling Interval</label>
                        <div className="flex flex-wrap items-center gap-4">
                            <div className="flex items-center gap-2 w-48 min-w-[140px]">
                                <Input
                                    type="number"
                                    min="1"
                                    max="1440"
                                    className="flex-1 min-w-0"
                                    value={localInterval}
                                    onChange={(e) => setLocalInterval(parseInt(e.target.value))}
                                    onBlur={handleIntervalBlur}
                                />
                                <span className="text-sm shrink-0">min</span>
                            </div>
                            <span className="text-sm text-muted-foreground">
                                (1 - 1440 minutes)
                            </span>
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>System Information</CardTitle>
                </CardHeader>
                <CardContent className="text-sm space-y-2">
                    <div className="flex justify-between">
                        <span className="text-muted-foreground">App Version</span>
                        <span>0.1.0</span>
                    </div>
                    <div className="flex justify-between">
                        <span className="text-muted-foreground">Tauri Version</span>
                        <span>v2.0.0</span>
                    </div>
                </CardContent>
            </Card>
        </div>
    );
}

