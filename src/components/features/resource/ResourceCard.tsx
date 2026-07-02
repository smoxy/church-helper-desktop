import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';
import { useI18n } from '../../../lib/i18n';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { FileHeadphone, FilePlay, FileText, MonitorPlay, Check, Download, LoaderCircle, Zap, Clock } from "lucide-react";

interface ResourceCardProps {
    resource: Resource;
    onClick: () => void;
}

export function ResourceCard({ resource, onClick }: ResourceCardProps) {
    const { t, lang } = useI18n();
    const { isDownloaded, isDownloading, isPending, queuePosition, revealInFolder } = useResource(resource);

    const getFileIcon = (type: string | null, isYoutube: boolean) => {
        if (isYoutube) return <MonitorPlay className="h-5 w-5 text-red-500" />;
        const t = type?.toLowerCase() || "";
        if (t.includes("audio") || t.includes("mp3")) return <FileHeadphone className="h-5 w-5 text-yellow-500" />;
        if (t.includes("video") || t.includes("mp4")) return <FilePlay className="h-5 w-5 text-blue-500" />;
        return <FileText className="h-5 w-5 text-gray-500" />;
    };

    const isYoutube = resource.download_url.includes("youtube.com") || resource.download_url.includes("youtu.be");

    return (
        <Card className="overflow-hidden cursor-pointer hover:border-primary/50 transition-colors group relative">
            {/* Stretched button: makes the whole card clickable/keyboard-focusable
                as a real <button>, kept as a SIBLING (not ancestor) of the
                download-status button below so no interactive element is
                nested inside another (illegal per ARIA). It sits at a lower
                z-index than the status indicator, so the indicator wins
                hit-testing over its own small area while this button catches
                clicks everywhere else on the card. */}
            <button
                type="button"
                onClick={onClick}
                aria-label={resource.title}
                className="absolute inset-0 z-10 rounded-lg cursor-pointer focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            />

            {/* Download Status Indicator (Top Right of Card) */}
            <div className="absolute top-2 right-2 z-20 flex gap-1">
                {resource.optimized_video_url && (
                    <div className="bg-green-500/90 text-white p-1.5 rounded-full shadow-xs" title={t('resourceCard.optimizedAvailable')}>
                        <Zap className="h-3 w-3" />
                    </div>
                )}
                {isDownloading ? (
                    <div className="bg-muted text-muted-foreground p-1.5 rounded-full shadow-xs" title={t('resourceCard.downloading')}>
                        <LoaderCircle className="h-3 w-3 animate-spin" />
                    </div>
                ) : isPending ? (
                    <div
                        className="bg-amber-500/90 text-white p-1.5 rounded-full shadow-xs"
                        title={queuePosition ? t('resourceCard.queuedAt', { position: queuePosition }) : t('resourceCard.queued')}
                    >
                        <Clock className="h-3 w-3" />
                    </div>
                ) : isDownloaded ? (
                    <button
                        type="button"
                        onClick={() => void revealInFolder()}
                        className="bg-success text-success-foreground p-1.5 rounded-full shadow-xs hover:bg-success/90 transition-colors cursor-pointer focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                        title={t('resourceCard.openInFolder')}
                        aria-label={t('resourceCard.openInFolder')}
                    >
                        <Check className="h-3 w-3" />
                    </button>
                ) : (
                    <div className="bg-black/30 text-white p-1.5 rounded-full opacity-0 group-hover:opacity-100 transition-opacity" title={t('resourceCard.clickToView')}>
                        <Download className="h-3 w-3" />
                    </div>
                )}
            </div>

            {resource.thumbnail_url && (
                <div className="aspect-video w-full overflow-hidden relative bg-muted">
                    <img
                        src={resource.thumbnail_url}
                        alt={resource.title}
                        loading="lazy"
                        decoding="async"
                        className="w-full h-full object-cover"
                    />
                    {/* Category Badge overlay on image */}
                    <div className="absolute bottom-2 left-2 bg-black/60 text-white text-[10px] px-2 py-0.5 rounded-sm uppercase font-bold tracking-wider">
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
                    <span className="text-muted-foreground">{new Date(resource.created_at).toLocaleDateString(lang === 'it' ? 'it-IT' : 'en-US')}</span>
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
