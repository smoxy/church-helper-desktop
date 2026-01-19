import { useEffect, useState } from "react";
import { useAppStore } from "../stores/appStore";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { RefreshCw, FileAudio, FileVideo, FileText, MonitorPlay } from "lucide-react";
import { format } from "date-fns";
import { Resource } from "../types";
import { ResourceDetail } from "../components/features/resource/ResourceDetail";

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

    const getFileIcon = (type: string | null, isYoutube: boolean) => {
        if (isYoutube) return <MonitorPlay className="h-5 w-5 text-red-500" />;

        // Simple heuristic based on type or extension if type is generic
        const t = type?.toLowerCase() || "";
        if (t.includes("audio") || t.includes("mp3")) return <FileAudio className="h-5 w-5 text-yellow-500" />;
        if (t.includes("video") || t.includes("mp4")) return <FileVideo className="h-5 w-5 text-blue-500" />;
        return <FileText className="h-5 w-5 text-gray-500" />;
    };

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
                            <Card
                                key={resource.id}
                                className="overflow-hidden cursor-pointer hover:border-primary/50 transition-colors group"
                                onClick={() => setSelectedResource(resource)}
                            >
                                {resource.thumbnail_url && (
                                    <div className="aspect-video w-full overflow-hidden">
                                        <img
                                            src={resource.thumbnail_url}
                                            alt={resource.title}
                                            className="w-full h-full object-cover transition-transform group-hover:scale-105"
                                        />
                                    </div>
                                )}
                                <CardHeader className="pb-3">
                                    <CardTitle className="text-lg line-clamp-1 group-hover:text-primary transition-colors" title={resource.title}>
                                        {resource.title}
                                    </CardTitle>
                                    <CardDescription className="flex items-center gap-2 text-xs">
                                        {getFileIcon(
                                            resource.file_type,
                                            resource.download_url.includes("youtube.com") || resource.download_url.includes("youtu.be")
                                        )}
                                        <span className="uppercase tracking-wider">{resource.category}</span>
                                    </CardDescription>
                                </CardHeader>
                                <CardContent>
                                    <p className="text-sm text-muted-foreground line-clamp-3">
                                        {resource.description}
                                    </p>
                                </CardContent>
                            </Card>
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
