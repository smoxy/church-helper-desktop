import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';
import { useI18n } from '../../../lib/i18n';
import { formatProjectionDate } from '../../../lib/projectionDate';
import { Card } from '../../ui/card';
import { ResourceMedia } from './ResourceMedia';
import { FileHeadphone, FilePlay, FileText, MonitorPlay, Check, Download, LoaderCircle, Zap, Clock } from "lucide-react";

interface ResourceCardProps {
    resource: Resource;
    onClick: () => void;
}

export function ResourceCard({ resource, onClick }: ResourceCardProps) {
    const { t, lang, categoryLabel } = useI18n();
    const { isDownloaded, isDownloading, isPending, queuePosition, revealInFolder } = useResource(resource);

    const getFileIcon = (type: string | null, isYoutube: boolean) => {
        if (isYoutube) return <MonitorPlay className="h-4 w-4 shrink-0 text-red-500" aria-hidden="true" />;
        const t = type?.toLowerCase() || "";
        if (t.includes("audio") || t.includes("mp3")) return <FileHeadphone className="h-4 w-4 shrink-0 text-yellow-500" aria-hidden="true" />;
        if (t.includes("video") || t.includes("mp4")) return <FilePlay className="h-4 w-4 shrink-0 text-blue-500" aria-hidden="true" />;
        return <FileText className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />;
    };

    const isYoutube = resource.download_url.includes("youtube.com") || resource.download_url.includes("youtu.be");
    const category = categoryLabel(resource.category);
    const shortDate = formatProjectionDate(resource, lang, 'short');
    const longDate = formatProjectionDate(resource, lang, 'long');

    return (
        <Card className="flex h-full flex-col overflow-hidden cursor-pointer hover:border-primary/50 transition-colors group relative">
            {/* Stretched button: makes the whole card clickable/keyboard-focusable
                as a real <button>, kept as a SIBLING (not ancestor) of the
                download-status button below so no interactive element is
                nested inside another (illegal per ARIA). It sits at a lower
                z-index than the status indicator, so the indicator wins
                hit-testing over its own small area while this button catches
                clicks everywhere else on the card. It also carries the full
                title as tooltip, since it covers the (clamped) title text. */}
            <button
                type="button"
                onClick={onClick}
                aria-label={resource.title}
                title={resource.title}
                className="absolute inset-0 z-10 rounded-lg cursor-pointer focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            />

            {/* Always-present 16:9 media area; status badges live on it, so
                they never overlap the title below. */}
            <ResourceMedia
                imageUrl={resource.thumbnail_url}
                alt={resource.title}
                category={resource.category}
                categoryLabel={category}
                showCategoryChip
            >
                <div className="absolute top-2 right-2 z-20 flex gap-1">
                    {resource.optimized_video_url && (
                        <div className="bg-green-600 text-white p-1.5 rounded-full shadow-xs" title={t('resourceCard.optimizedAvailable')}>
                            <Zap className="h-3 w-3" />
                        </div>
                    )}
                    {isDownloading ? (
                        <div className="bg-muted text-muted-foreground p-1.5 rounded-full shadow-xs" title={t('resourceCard.downloading')}>
                            <LoaderCircle className="h-3 w-3 animate-spin" />
                        </div>
                    ) : isPending ? (
                        <div
                            className="bg-amber-600 text-white p-1.5 rounded-full shadow-xs"
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
                        <div className="bg-black/70 text-white p-1.5 rounded-full opacity-0 group-hover:opacity-100 transition-opacity" title={t('resourceCard.clickToView')}>
                            <Download className="h-3 w-3" />
                        </div>
                    )}
                </div>
            </ResourceMedia>

            {/* Fixed minimum body height (2-line title + footer); grid rows
                stretch cards, so a description only grows the whole row. */}
            <div className="flex min-h-[6.5rem] flex-1 flex-col gap-2 p-4">
                <h3 className="min-h-[2.75rem] text-base font-semibold leading-snug line-clamp-2 break-words group-hover:text-primary transition-colors">
                    {resource.title}
                </h3>
                {resource.description && (
                    <p className="text-sm text-muted-foreground line-clamp-2">
                        {resource.description}
                    </p>
                )}
                <div className="mt-auto flex items-center gap-2 text-xs text-muted-foreground">
                    {getFileIcon(resource.file_type, isYoutube)}
                    {shortDate && (
                        <time dateTime={resource.week_date ?? undefined} title={longDate ?? undefined}>
                            {shortDate}
                        </time>
                    )}
                </div>
            </div>
        </Card>
    );
}
