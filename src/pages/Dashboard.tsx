import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { useToastStore } from "../stores/toastStore";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { RefreshCw, FileText } from "lucide-react";
import { format } from "date-fns";
import { Resource } from "../types";
import { ResourceDetail } from "../components/features/resource/ResourceDetail";
import { ResourceCard } from "../components/features/resource/ResourceCard";
import { ResourcesFoundCard } from "../components/features/resource/ResourcesFoundCard";
import { DownloadsModal } from "../components/features/resource/DownloadsModal";

export default function Dashboard() {
    const {
        status,
        resources,
        isLoading,
        error,
        activeDownloads,
        fetchInitialData,
        forcePoll
    } = useAppStore();

    const { addToast } = useToastStore();
    const [selectedResource, setSelectedResource] = useState<Resource | null>(null);
    const [showDownloads, setShowDownloads] = useState(false);

    useEffect(() => {
        fetchInitialData();
    }, [fetchInitialData]);

    const handleRefresh = async () => {
        try {
            await forcePoll();
            addToast("Week Material updated", "success");
        } catch (e) {
            addToast(`Refresh failed: ${e}`, "error");
        }
    };

    useEffect(() => {
        if (error) {
            addToast(`Error: ${error}`, "error");
        }
    }, [error, addToast]);

    const activeDownloadsCount = Object.values(activeDownloads).filter(d => d.status === 'downloading' || d.status === 'pending').length;

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <div>
                    <h2 className="text-3xl font-bold tracking-tight">Dashboard</h2>
                    <p className="text-muted-foreground mt-1">
                        Overview of this week's resources and application status.
                    </p>
                </div>
            </div>

            {/* Status Cards */}
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
                <ResourcesFoundCard
                    resourceCount={resources.length}
                    activeDownloadsCount={activeDownloadsCount}
                    onClick={() => setShowDownloads(true)}
                />

                <Card>
                    <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                        <CardTitle className="text-sm font-medium">Current Week</CardTitle>
                        <div className="h-4 w-4 text-muted-foreground text-xs font-bold border rounded flex items-center justify-center">W</div>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {status?.current_week ? `${status.current_week.year}-W${status.current_week.week_number}` : "--"}
                        </div>
                        <p className="text-xs text-muted-foreground">
                            Latest available content
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                        <CardTitle className="text-sm font-medium">Polling Status</CardTitle>
                        <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-primary transition-colors"
                            onClick={handleRefresh}
                            disabled={isLoading}
                        >
                            <RefreshCw className={`h-4 w-4 ${isLoading ? "animate-spin" : ""}`} />
                        </Button>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold flex items-center gap-2">
                            <span className={`h-3 w-3 rounded-full ${status?.polling_active ? "bg-green-500" : "bg-gray-300"}`}></span>
                            {status?.polling_active ? "Active" : "Paused"}
                        </div>
                        <p className="text-xs text-muted-foreground">
                            {status?.last_poll_time
                                ? `Last update: ${format(new Date(status.last_poll_time), "HH:mm")}`
                                : "No updates yet"
                            }
                        </p>
                    </CardContent>
                </Card>
            </div>

            {/* Resources List */}
            <div className="space-y-4">
                <h3 className="text-xl font-semibold">Weekly Material</h3>
                <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                    {resources.length === 0 ? (
                        <div className="col-span-full text-center py-10 border rounded-lg bg-card text-muted-foreground">
                            No resources found for the current week.
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
