import { Card, CardContent, CardHeader, CardTitle } from "../../ui/card";
import { FileText, Download, CheckCircle2, CloudDownload } from "lucide-react";
import { ResourceSummary, ActiveDownload } from "../../../stores/appStore";

interface ResourcesFoundCardProps {
    summary: ResourceSummary | null;
    activeDownloads: Record<number, ActiveDownload>;
    onClick: () => void;
}

export function ResourcesFoundCard({
    summary,
    activeDownloads,
    onClick
}: ResourcesFoundCardProps) {
    if (!summary) return (
        <Card className="cursor-pointer hover:bg-accent/50 transition-colors animate-pulse">
            <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium opacity-50">Resources Found</CardTitle>
            </CardHeader>
            <CardContent>
                <div className="h-8 w-24 bg-muted rounded"></div>
            </CardContent>
        </Card>
    );

    // Determine aggregate state for the status bar
    const downloads = Object.values(activeDownloads);
    const hasError = downloads.some(d => d.status === 'error');
    const hasPaused = downloads.some(d => d.status === 'paused');
    const isDownloading = summary.active > 0;

    let barColor = "bg-primary";
    let barBgColor = "bg-primary/20";
    let barAnimation = "animate-progress-indeterminate";
    let shouldShowBar = false;

    if (hasError) {
        barColor = "bg-destructive";
        barBgColor = "bg-destructive/20";
        barAnimation = ""; // Static error state
        shouldShowBar = true;
    } else if (hasPaused) {
        barColor = "bg-yellow-500";
        barBgColor = "bg-yellow-500/20";
        barAnimation = "opacity-50"; // Pulsing or static for paused
        shouldShowBar = true;
    } else if (isDownloading) {
        barColor = "bg-blue-600";
        barBgColor = "bg-blue-600/20";
        barAnimation = "animate-progress-indeterminate";
        shouldShowBar = true;
    }

    return (
        <Card
            className="cursor-pointer hover:bg-accent/50 transition-all relative overflow-hidden group border-primary/20 h-full"
            onClick={onClick}
        >
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-semibold tracking-tight text-muted-foreground uppercase">Material Summary</CardTitle>
                <FileText className="h-4 w-4 text-muted-foreground group-hover:text-primary transition-colors" />
            </CardHeader>
            <CardContent>
                <div className="flex items-center justify-between w-full">
                    {/* Total Found */}
                    <div className="flex flex-col">
                        <div className="flex items-baseline gap-1">
                            <span className="text-2xl font-black text-foreground tracking-tighter">{summary.total}</span>
                            <span className="text-[10px] font-bold text-muted-foreground uppercase opacity-70">Found</span>
                        </div>
                    </div>

                    <div className="h-8 w-px bg-border/60"></div>

                    {/* Successfully Downloaded */}
                    <div className="flex flex-col">
                        <div className="flex items-baseline gap-1">
                            <div className="flex items-center gap-1.5">
                                <span className={`text-2xl font-black tracking-tighter ${summary.downloaded === summary.total && summary.total > 0 ? 'text-green-600' : 'text-green-600'}`}>
                                    {summary.downloaded}
                                </span>
                                <CheckCircle2 className={`h-4 w-4 ${summary.downloaded === summary.total && summary.total > 0 ? 'text-green-600' : 'text-green-600/50'}`} />
                            </div>
                            <span className="text-[10px] font-bold text-muted-foreground uppercase opacity-70">Ready</span>
                        </div>
                    </div>

                    <div className="h-8 w-px bg-border/60"></div>

                    {/* Currently Downloading / Active */}
                    <div className="flex flex-col">
                        <div className="flex items-baseline gap-1">
                            <div className="flex items-center gap-1.5">
                                <span className={`text-2xl font-black tracking-tighter ${summary.active > 0 ? 'text-blue-600' : 'text-foreground opacity-40'}`}>
                                    {summary.active}
                                </span>
                                {summary.active > 0 ? (
                                    <Download className="h-5 w-5 text-blue-600" />
                                ) : (
                                    <CloudDownload className="h-4 w-4 text-muted-foreground/30" />
                                )}
                            </div>
                            <span className="text-[10px] font-bold text-muted-foreground uppercase opacity-70">Downloading</span>
                        </div>
                    </div>
                </div>
            </CardContent>

            {/* Active state indicator line at the bottom */}
            {shouldShowBar && (
                <div className={`absolute inset-x-0 bottom-0 h-1 ${barBgColor}`}>
                    <div className={`h-full ${barColor} ${barAnimation} origin-left`} />
                </div>
            )}
        </Card>
    );
}
