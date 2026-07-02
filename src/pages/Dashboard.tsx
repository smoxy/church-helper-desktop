import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { useToastStore } from "../stores/toastStore";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { RefreshCw } from "lucide-react";
import { format } from "date-fns";
import { errorMessage } from "../lib/utils";
import { useI18n } from "../lib/i18n";
import { Resource } from "../types";
import { ResourceDetail } from "../components/features/resource/ResourceDetail";
import { ResourceCard } from "../components/features/resource/ResourceCard";
import { ResourcesFoundCard } from "../components/features/resource/ResourcesFoundCard";
import { DownloadsModal } from "../components/features/resource/DownloadsModal";
import { StaleWeekBanner } from "../components/features/dashboard/StaleWeekBanner";

export default function Dashboard() {
    const { t } = useI18n();
    const status = useAppStore(s => s.status);
    const resources = useAppStore(s => s.resources);
    const isLoading = useAppStore(s => s.isLoading);
    const error = useAppStore(s => s.error);
    const summary = useAppStore(s => s.summary);
    const fetchInitialData = useAppStore(s => s.fetchInitialData);
    const forcePoll = useAppStore(s => s.forcePoll);

    const { addToast } = useToastStore();
    const [selectedResource, setSelectedResource] = useState<Resource | null>(null);
    const [showDownloads, setShowDownloads] = useState(false);

    useEffect(() => {
        fetchInitialData();
    }, [fetchInitialData]);

    const handleRefresh = async () => {
        try {
            await forcePoll();
            addToast(t('dashboard.toast.refreshSuccess'), "success");
        } catch (e) {
            addToast(t('dashboard.toast.refreshError', { error: errorMessage(e) }), "error");
        }
    };

    useEffect(() => {
        if (error) {
            addToast(t('dashboard.toast.genericError', { error }), "error");
        }
    }, [error, addToast, t]);



    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <div>
                    <h2 className="text-3xl font-bold tracking-tight">{t('dashboard.title')}</h2>
                    <p className="text-muted-foreground mt-1">
                        {t('dashboard.subtitle')}
                    </p>
                </div>
            </div>

            {status?.material_week_stale && (
                <StaleWeekBanner currentWeek={status.current_week} />
            )}

            {/* Status Cards */}
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
                <div className="lg:col-span-2">
                    <ResourcesFoundCard
                        summary={summary}
                        onClick={() => setShowDownloads(true)}
                    />
                </div>

                <Card>
                    <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                        <CardTitle className="text-sm font-medium">{t('dashboard.currentWeek')}</CardTitle>
                        <div className="h-4 w-4 text-muted-foreground text-xs font-bold border rounded flex items-center justify-center">W</div>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {status?.current_week ? `${status.current_week.year}-W${status.current_week.week_number}` : "--"}
                        </div>
                        <p className="text-xs text-muted-foreground">
                            {t('dashboard.currentWeek.hint')}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                        <CardTitle className="text-sm font-medium">{t('dashboard.pollingStatus')}</CardTitle>
                        <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-primary transition-colors"
                            onClick={handleRefresh}
                            disabled={isLoading}
                            aria-label={t('dashboard.refresh')}
                        >
                            <RefreshCw className={`h-4 w-4 ${isLoading ? "animate-spin" : ""}`} />
                        </Button>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold flex items-center gap-2">
                            <span className={`h-3 w-3 rounded-full ${status?.polling_active ? "bg-green-500" : "bg-gray-300"}`}></span>
                            {status?.polling_active ? t('dashboard.pollingActive') : t('dashboard.pollingPaused')}
                        </div>
                        <p className="text-xs text-muted-foreground">
                            {status?.last_poll_time
                                ? t('dashboard.lastUpdate', { time: format(new Date(status.last_poll_time), "HH:mm") })
                                : t('dashboard.noUpdatesYet')
                            }
                        </p>
                    </CardContent>
                </Card>
            </div>

            {/* Resources List */}
            <div className="space-y-4">
                <h3 className="text-xl font-semibold">{t('dashboard.weeklyMaterial')}</h3>
                <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                    {resources.length === 0 ? (
                        <div className="col-span-full text-center py-10 border rounded-lg bg-card text-muted-foreground">
                            {t('dashboard.noResourcesFound')}
                        </div>
                    ) : (
                        resources.map((resource) => (
                            <ResourceCard
                                key={resource.id}
                                resource={resource}
                                onClick={() => setSelectedResource(resource)}
                            />
                        ))
                    )}
                </div>
            </div>

            {selectedResource && (
                <ResourceDetail
                    resource={selectedResource}
                    onClose={() => setSelectedResource(null)}
                />
            )}

            <DownloadsModal
                open={showDownloads}
                onClose={() => setShowDownloads(false)}
            />
        </div>
    );
}
