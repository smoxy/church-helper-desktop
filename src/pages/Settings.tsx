import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Switch } from "../components/ui/switch";
import { FolderOpen, Save } from "lucide-react";


export default function Settings() {
    const {
        config,
        fetchInitialData,
        selectWorkDirectory,
        togglePolling,
        setPollingInterval,
        setRetentionDays
    } = useAppStore();

    // Local state for interval to manage input changes before committing
    const [localInterval, setLocalInterval] = useState(60);
    const [localRetention, setLocalRetention] = useState<number | null>(null);

    useEffect(() => {
        fetchInitialData();
    }, [fetchInitialData]);

    useEffect(() => {
        if (config) {
            setLocalInterval(config.polling_interval_minutes);
            setLocalRetention(config.retention_days);
        }
    }, [config]);

    const handleIntervalChange = async () => {
        await setPollingInterval(localInterval);
    };

    const handleRetentionChange = async () => {
        await setRetentionDays(localRetention);
    };

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
                        <div className="flex items-center gap-4">
                            <div className="flex-1">
                                <div className="flex items-center gap-2">
                                    <Input
                                        type="number"
                                        min="0"
                                        placeholder="Days (Empty = Forever)"
                                        value={localRetention === null ? "" : localRetention}
                                        onChange={(e) => {
                                            const val = e.target.value;
                                            setLocalRetention(val === "" ? null : parseInt(val));
                                        }}
                                    />
                                    <Button size="sm" onClick={handleRetentionChange}>
                                        <Save className="h-4 w-4" />
                                    </Button>
                                </div>
                            </div>
                            <span className="text-sm text-muted-foreground">
                                days to keep archives (0 = delete immediately, empty = keep forever)
                            </span>
                        </div>
                    </div>
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
                    <div className="flex items-center justify-between">
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
                        <label className="text-sm font-medium">Polling Interval</label>
                        <div className="flex items-center gap-4">
                            <div className="flex items-center gap-2 w-48">
                                <Input
                                    type="number"
                                    min="1"
                                    max="1440"
                                    value={localInterval}
                                    onChange={(e) => setLocalInterval(parseInt(e.target.value))}
                                />
                                <span className="text-sm">min</span>
                            </div>
                            <Button size="sm" variant="outline" onClick={handleIntervalChange}>
                                Save Interval
                            </Button>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            How often to check for updates (1 - 1440 minutes).
                        </p>
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
