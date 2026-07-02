import { useEffect, useMemo, useRef } from 'react';
import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';
import { useResourceFileSize } from '../../../hooks/useResourceFileSize';
import { formatBytes } from '../../../lib/utils';
import { computeSavings } from '../../../lib/savings';
import { OptimizedVideoPicker } from './OptimizedVideoPicker';
import { OptimizedPreferenceToggle } from './OptimizedPreferenceToggle';
import { LoaderCircle, Download, Check, Pause, Play, Trash2, TriangleAlert, RotateCcw, X, FolderOpen, Clock } from "lucide-react";

interface ResourceDetailProps {
    resource: Resource;
    onClose: () => void;
}

export function ResourceDetail({ resource, onClose }: ResourceDetailProps) {
    const {
        isDownloaded,
        isDownloading,
        isPaused,
        isPending,
        queuePosition,
        isAutoDownloadEnabled,
        error,
        progress,
        integrity,
        download,
        pause,
        resume,
        cancel,
        revealInFolder,
        toggleAutoDownload,
        preferOptimized,
        setPreferOptimized,
        hasOptimizedVariant,
        optimizedVideos,
        selectedVideoUrl,
        selectVideo
    } = useResource(resource);

    // Sizes: batched from the store when available, otherwise fetched lazily
    // (HEAD) only here in the detail view (and when the variant changes).
    const { originalSizeBytes, optimizedSizeBytes } =
        useResourceFileSize(resource, selectedVideoUrl);

    const fileSize = useMemo(() => {
        const bytes = preferOptimized && optimizedSizeBytes
            ? optimizedSizeBytes
            : originalSizeBytes;
        return bytes ? formatBytes(bytes) : null;
    }, [preferOptimized, originalSizeBytes, optimizedSizeBytes]);

    const closeButtonRef = useRef<HTMLButtonElement>(null);

    useEffect(() => {
        closeButtonRef.current?.focus();
    }, []);

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") onClose();
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [onClose]);

    const handleMainAction = () => {
        if (isDownloaded && !isCorrupted) {
            void revealInFolder();
        } else if (isDownloading) {
            void pause();
        } else if (isPaused) {
            void resume();
        } else if (isPending) {
            // Queued: no action, button is disabled.
            return;
        } else {
            void download();
        }
    };

    const isCorrupted = isDownloaded && integrity === 'mismatch';

    // adr-0008: only show the picker when the user wants optimized videos at
    // all (matches useResource's own gating) AND there is an actual choice
    // to make; with 0/1 elements the download button behaves exactly as
    // before (single implicit URL, no picker rendered).
    const showVideoPicker = preferOptimized && optimizedVideos.length > 1;

    // Intervento A: the callout appears whenever the user hasn't opted into
    // optimized videos yet AND this resource actually offers one, regardless
    // of whether the sizes are known — with numbers it shows the real saving,
    // without it falls back to generic copy (see OptimizedPreferenceToggle).
    const showOptimizedCallout = !preferOptimized && hasOptimizedVariant;
    const savings = computeSavings(originalSizeBytes, optimizedSizeBytes);
    const originalLabel = originalSizeBytes ? formatBytes(originalSizeBytes) : null;
    const optimizedLabel = optimizedSizeBytes ? formatBytes(optimizedSizeBytes) : null;

    return (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center p-4 z-50 backdrop-blur-xs" onClick={onClose}>
            <div role="dialog" aria-modal="true" aria-label={resource.title} className="bg-card text-card-foreground rounded-xl shadow-2xl max-w-2xl w-full p-6 animate-in fade-in zoom-in duration-200" onClick={e => e.stopPropagation()}>
                <div className="flex justify-between items-start mb-6">
                    <h2 className="text-2xl font-bold text-primary">{resource.title}</h2>
                    <button ref={closeButtonRef} onClick={onClose} aria-label="Close" className="text-muted-foreground hover:text-foreground">
                        <X className="h-6 w-6" />
                    </button>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                    <div>
                        {resource.thumbnail_url ? (
                            <img src={resource.thumbnail_url} alt={resource.title} loading="lazy" decoding="async" className="w-full h-auto rounded-lg shadow-md mb-4 object-cover aspect-video" />
                        ) : (
                            <div className="w-full h-48 bg-muted rounded-lg mb-4 flex items-center justify-center text-muted-foreground">
                                No Thumbnail
                            </div>
                        )}

                        <div className="flex flex-col gap-4 mt-6">
                            <div className="w-full">
                                <div className="flex justify-between items-center mb-2">
                                    <h3 className="text-lg font-semibold">Download</h3>
                                    <div className="flex items-center gap-2">
                                        {fileSize && !error && (
                                            <span className="text-sm text-muted-foreground font-medium bg-muted/50 px-2 py-0.5 rounded">
                                                {fileSize}
                                            </span>
                                        )}
                                    </div>
                                </div>

                                <div className="flex flex-col gap-3">
                                    {/* Intervento A: inline callout offering to switch prefer_optimized on,
                                        replacing the former "check Settings" tooltip. Gate is on
                                        hasOptimizedVariant (not on known sizes), so it shows generic
                                        copy when sizes are unknown and real numbers when known. */}
                                    {showOptimizedCallout && (
                                        <OptimizedPreferenceToggle
                                            savings={savings}
                                            originalLabel={originalLabel}
                                            optimizedLabel={optimizedLabel}
                                            onEnable={() => void setPreferOptimized(true)}
                                        />
                                    )}

                                    {/* Optimized video picker (adr-0008): only when there's an actual choice */}
                                    {showVideoPicker && (
                                        <OptimizedVideoPicker
                                            videos={optimizedVideos}
                                            selectedUrl={selectedVideoUrl}
                                            onSelect={selectVideo}
                                            disabled={isDownloading || isPaused || (isDownloaded && !isCorrupted)}
                                        />
                                    )}

                                    {/* Progress Bar Area */}
                                    {(isDownloading || isPaused) && (
                                        <div className="w-full bg-muted rounded-full h-2.5 overflow-hidden">
                                            <div
                                                className={`h-full transition-all duration-300 ease-out ${isPaused ? 'bg-yellow-500' : 'bg-primary'}`}
                                                style={{ width: `${progress || 0}%` }}
                                            />
                                        </div>
                                    )}

                                    <div className="flex items-center gap-2">
                                        {/* Main Action Button */}
                                        <button
                                            onClick={handleMainAction}
                                            disabled={isPending}
                                            className={`
                                                relative flex items-center justify-center gap-2 px-6 py-3 rounded-lg font-bold transition-all shadow-md active:scale-95 flex-1
                                                ${isCorrupted ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90' :
                                                    isPaused ? 'bg-yellow-500 text-white hover:bg-yellow-600' :
                                                        isDownloading ? 'bg-muted text-muted-foreground border-2 border-primary/20' :
                                                            isPending ? 'bg-amber-500/10 text-amber-600 border-2 border-amber-500/30 cursor-not-allowed' :
                                                                isDownloaded ? 'bg-success text-success-foreground cursor-pointer hover:bg-success/90' :
                                                                    'bg-primary text-primary-foreground hover:bg-primary/90'}
                                            `}
                                            title={isCorrupted ? "File corrupted. Click to retry." :
                                                isDownloaded ? "Apri nella cartella" :
                                                    isPending ? (queuePosition ? `In coda (posizione ${queuePosition})` : "In coda") : ""}
                                            aria-label={isDownloaded && !isCorrupted ? "Downloaded. Apri nella cartella" : undefined}
                                        >
                                            {isDownloading ? (
                                                <>
                                                    <LoaderCircle className="h-5 w-5 animate-spin" />
                                                    <span className="font-mono">{progress}%</span>
                                                    <Pause className="h-4 w-4 opacity-70 ml-1" />
                                                </>
                                            ) : isPaused ? (
                                                <>
                                                    <Play className="h-5 w-5 fill-current" />
                                                    Resume ({progress}%)
                                                </>
                                            ) : isPending ? (
                                                <>
                                                    <Clock className="h-5 w-5" />
                                                    {queuePosition ? `In coda (${queuePosition}º)` : "In coda"}
                                                </>
                                            ) : isCorrupted ? (
                                                <>
                                                    <RotateCcw className="h-5 w-5" />
                                                    Retry Download
                                                </>
                                            ) : isDownloaded ? (
                                                <>
                                                    <Check className="h-5 w-5" />
                                                    Downloaded
                                                </>
                                            ) : (
                                                <>
                                                    <Download className="h-5 w-5" />
                                                    Download Now
                                                </>
                                            )}
                                        </button>

                                        {/* Stop/Cancel Button: also shown while queued so a
                                            pending download can be removed from the queue
                                            (cancelDownload calls the backend's remove_queued
                                            for pending items). */}
                                        {(isDownloading || isPaused || isPending) && (
                                            <button
                                                onClick={cancel}
                                                className="p-3 bg-muted text-muted-foreground rounded-lg hover:bg-destructive hover:text-destructive-foreground transition-colors shadow-xs"
                                                title={isPending ? "Remove from queue" : "Stop and Delete"}
                                            >
                                                <Trash2 className="h-5 w-5" />
                                            </button>
                                        )}
                                    </div>

                                    {/* Reveal in file manager: only once the file is on disk */}
                                    {isDownloaded && !isCorrupted && (
                                        <button
                                            onClick={() => void revealInFolder()}
                                            className="flex items-center justify-center gap-2 px-4 py-2 rounded-lg text-sm font-medium border border-border bg-card text-foreground hover:bg-muted transition-colors active:scale-95"
                                        >
                                            <FolderOpen className="h-4 w-4" />
                                            Apri nella cartella
                                        </button>
                                    )}

                                    {/* Warnings / Errors */}
                                    {isCorrupted && (
                                        <div className="flex items-center gap-2 text-xs text-destructive font-semibold bg-destructive/10 p-2 rounded">
                                            <TriangleAlert className="h-4 w-4" />
                                            Hashes do not match source. File may be corrupted.
                                        </div>
                                    )}

                                    {error && (
                                        <div className="w-full text-xs text-destructive font-medium bg-destructive/10 p-2 rounded animate-in fade-in slide-in-from-top-1">
                                            {error === "Work directory not configured"
                                                ? "Please set a download folder in Settings."
                                                : error}
                                        </div>
                                    )}
                                </div>
                            </div>
                        </div>
                    </div>

                    <div className="space-y-4">
                        <div className="flex justify-between items-start">
                            <div>
                                <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">Category</span>
                                <p className="text-lg font-medium text-foreground capitalize">{resource.category}</p>
                            </div>

                            {/* Auto-download Toggle with Tooltip */}
                            <div className="flex flex-col items-end gap-1.5 pt-1">
                                <div className="flex items-center gap-2">
                                    <span className={`text-[10px] font-black uppercase tracking-widest transition-colors ${isAutoDownloadEnabled ? 'text-success' : 'text-muted-foreground/60'}`}>
                                        Auto-download
                                    </span>
                                    {isAutoDownloadEnabled && (
                                        <span className="flex h-1.5 w-1.5 rounded-full bg-success animate-pulse" />
                                    )}
                                </div>
                                <div className="group relative flex items-center">
                                    <button
                                        onClick={toggleAutoDownload}
                                        className={`
                                            relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-all duration-300 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                                            ${isAutoDownloadEnabled
                                                ? 'bg-success ring-4 ring-success/30 shadow-[0_0_15px_-3px_rgba(34,197,94,0.6)]'
                                                : 'bg-gray-200 dark:bg-gray-700 border border-transparent'}
                                        `}
                                    >
                                        <span className={`
                                            inline-block h-4 w-4 transform rounded-full bg-white shadow-xs transition-transform duration-300
                                            ${isAutoDownloadEnabled ? 'translate-x-6' : 'translate-x-1'}
                                        `} />
                                    </button>

                                    {/* Tooltip */}
                                    <div className="absolute right-0 top-full mt-2 w-48 p-2 bg-popover text-popover-foreground text-xs rounded-md shadow-lg opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-200 z-50 border border-border">
                                        Automatically download future resources in the <strong>{resource.category}</strong> category.
                                        <div className="absolute -top-1 right-3 w-2 h-2 bg-popover transform rotate-45 border-t border-l border-border"></div>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <div>
                            <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">Date</span>
                            <p className="text-base text-foreground">
                                {new Date(resource.created_at).toLocaleDateString(undefined, {
                                    weekday: 'long',
                                    year: 'numeric',
                                    month: 'long',
                                    day: 'numeric'
                                })}
                            </p>
                        </div>

                        <div>
                            <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">Description</span>
                            <p className="text-base text-foreground whitespace-pre-wrap mt-1 leading-relaxed">
                                {resource.description || "No description available."}
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
