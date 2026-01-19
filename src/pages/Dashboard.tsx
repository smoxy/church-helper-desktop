import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { RefreshCw, FileText } from "lucide-react";
import { format } from "date-fns";
import { Resource } from "../types";
import { ResourceDetail } from "../components/features/resource/ResourceDetail";
import { ResourceCard } from "../components/features/resource/ResourceCard";

export default function Dashboard() {
    const {
        status,
        resources,
        isLoading,
        error,
        fetchInitialData,
        forcePoll
    } = useAppStore();

    const [selectedResource, setSelectedResource] = useState<Resource | null>(null);

    useEffect(() => {
        fetchInitialData();
    }, [fetchInitialData]);

    if (error) {
        return (
            <div className="flex flex-col items-center justify-center p-8 space-y-4 text-center">
                <div className="text-destructive font-bold text-lg">Error</div>
                <p className="text-muted-foreground">{error}</p>
                <Button onClick={() => fetchInitialData()}>Retry</Button>
            </div>
        )
    }

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <div>
                    <h2 className="text-3xl font-bold tracking-tight">Dashboard</h2>
                    <p className="text-muted-foreground mt-1">
                        Overview of this week's resources and application status.
                    </p>
                </div>
                <Button
                    onClick={forcePoll}
                    disabled={isLoading}
                    variant="outline"
                    className="gap-2"
                >
                    <RefreshCw className={`h-4 w-4 ${isLoading ? "animate-spin" : ""}`} />
                    {isLoading ? "Refreshing..." : "Refresh Now"}
                </Button>
            </div>

            {/* Status Cards */}
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
                <Card>
                    <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                        <CardTitle className="text-sm font-medium">Resources Found</CardTitle>
                        <FileText className="h-4 w-4 text-muted-foreground" />
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">{resources.length}</div>
                        <p className="text-xs text-muted-foreground">
                            Total items for this week
                        </p>
                    </CardContent>
                </Card>

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
                        <RefreshCw className="h-4 w-4 text-muted-foreground" />
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
        </div>
    );
}
