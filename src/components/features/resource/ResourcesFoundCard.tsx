import { Card, CardContent, CardHeader, CardTitle } from "../../ui/card";
import { FileText, Loader2 } from "lucide-react";
import { cn } from "../../../lib/utils";

interface ResourcesFoundCardProps {
    resourceCount: number;
    activeDownloadsCount: number;
    onClick: () => void;
}

export function ResourcesFoundCard({
    resourceCount,
    activeDownloadsCount,
    onClick
}: ResourcesFoundCardProps) {
    return (
        <Card
            className="cursor-pointer hover:bg-accent/50 transition-colors relative overflow-hidden group"
            onClick={onClick}
        >
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Resources Found</CardTitle>
                <FileText className="h-4 w-4 text-muted-foreground group-hover:text-primary transition-colors" />
            </CardHeader>
            <CardContent>
                <div className="flex justify-between items-end">
                    <div>
                        <div className="text-2xl font-bold">{resourceCount}</div>
                        <p className="text-xs text-muted-foreground">
                            Total items for this week
                        </p>
                    </div>

                    {activeDownloadsCount > 0 && (
                        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 text-primary text-xs font-medium animate-in fade-in slide-in-from-right-2 duration-300">
                            <Loader2 className="h-3 w-3 animate-spin" />
                            <span>{activeDownloadsCount} downloading</span>
                        </div>
                    )}
                </div>
            </CardContent>

            {/* Active state indicator */}
            {activeDownloadsCount > 0 && (
                <div className="absolute inset-x-0 bottom-0 h-1 bg-primary/20">
                    <div className="h-full bg-primary animate-progress-indeterminate origin-left" />
                </div>
            )}
        </Card>
    );
}
