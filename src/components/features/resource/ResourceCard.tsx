import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { FileAudio, FileVideo, FileText, MonitorPlay, Check, Download, Loader2 } from "lucide-react";

interface ResourceCardProps {
    resource: Resource;
    onClick: () => void;
}

export function ResourceCard({ resource, onClick }: ResourceCardProps) {
    const { isDownloaded, isDownloading } = useResource(resource);

    const getFileIcon = (type: string | null, isYoutube: boolean) => {
        if (isYoutube) return <MonitorPlay className="h-5 w-5 text-red-500" />;
        const t = type?.toLowerCase() || "";
        if (t.includes("audio") || t.includes("mp3")) return <FileAudio className="h-5 w-5 text-yellow-500" />;
        if (t.includes("video") || t.includes("mp4")) return <FileVideo className="h-5 w-5 text-blue-500" />;
        return <FileText className="h-5 w-5 text-gray-500" />;
    };

    const isYoutube = resource.download_url.includes("youtube.com") || resource.download_url.includes("youtu.be");

    return (
        <Card
            className="overflow-hidden cursor-pointer hover:border-primary/50 transition-colors group relative"
            onClick={onClick}
        >
            {/* Download Status Indicator (Top Right of Card) */}
            <div className="absolute top-2 right-2 z-10 flex gap-1">
                {isDownloaded && (
                    <div className="bg-success text-success-foreground p-1.5 rounded-full shadow-sm" title="Downloaded">
                        <Check className="h-3 w-3" />
                    </div>
                )}
                {isDownloading && (
                    <div className="bg-muted text-muted-foreground p-1.5 rounded-full shadow-sm" title="Downloading...">
                        <Loader2 className="h-3 w-3 animate-spin" />
                    </div>
                )}
                {!isDownloaded && !isDownloading && (
                    <div className="bg-black/30 text-white p-1.5 rounded-full opacity-0 group-hover:opacity-100 transition-opacity" title="Click to view">
                        <Download className="h-3 w-3" />
                    </div>
                )}
            </div>

            {resource.thumbnail_url && (
                <div className="aspect-video w-full overflow-hidden relative bg-muted">
                    <img
                        src={resource.thumbnail_url}
                        alt={resource.title}
                        className="w-full h-full object-cover transition-transform group-hover:scale-105"
                    />
                    {/* Category Badge overlay on image */}
                    <div className="absolute bottom-2 left-2 bg-black/60 text-white text-[10px] px-2 py-0.5 rounded-sm uppercase font-bold tracking-wider backdrop-blur-sm">
                        {resource.category}
                    </div>
                </div>
            )}

            <CardHeader className="pb-3 pt-4">
                <CardTitle className="text-lg line-clamp-1 group-hover:text-primary transition-colors" title={resource.title}>
                    {resource.title}
                </CardTitle>
                <CardDescription className="flex items-center gap-2 text-xs">
                    {getFileIcon(resource.file_type, isYoutube)}
                    <span className="text-muted-foreground">{new Date(resource.created_at).toLocaleDateString()}</span>
                </CardDescription>
            </CardHeader>
            <CardContent>
                <p className="text-sm text-muted-foreground line-clamp-2">
                    {resource.description}
                </p>
            </CardContent>
        </Card>
    );
}
