import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';
import { Loader2, Download, Check, Pause, Play, Trash2, AlertTriangle, RotateCcw, X } from "lucide-react";

interface ResourceDetailProps {
    resource: Resource;
    onClose: () => void;
}

export function ResourceDetail({ resource, onClose }: ResourceDetailProps) {
    const {
        isDownloaded,
        isDownloading,
        isPaused,
        isAutoDownloadEnabled,
        fileSize,
        error,
        progress,
        integrity,
        download,
        pause,
        resume,
        cancel,
        toggleAutoDownload
    } = useResource(resource);

    const handleMainAction = () => {
        if (isDownloading) {
            pause();
        } else if (isPaused) {
            resume();
        } else {
            download();
        }
    };

    const isCorrupted = isDownloaded && integrity === 'mismatch';

    return (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center p-4 z-50 backdrop-blur-sm" onClick={onClose}>
            <div className="bg-card text-card-foreground rounded-xl shadow-2xl max-w-2xl w-full p-6 animate-in fade-in zoom-in duration-200" onClick={e => e.stopPropagation()}>
                <div className="flex justify-between items-start mb-6">
                    <h2 className="text-2xl font-bold text-primary">{resource.title}</h2>
                    <button onClick={onClose} className="text-muted-foreground hover:text-foreground">
                        <X className="h-6 w-6" />
                    </button>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                    <div>
                        {resource.thumbnail_url ? (
                            <img src={resource.thumbnail_url} alt={resource.title} className="w-full h-auto rounded-lg shadow-md mb-4 object-cover aspect-video" />
                        ) : (
                            <div className="w-full h-48 bg-muted rounded-lg mb-4 flex items-center justify-center text-muted-foreground">
                                No Thumbnail
                            </div>
                        )}

                        <div className="flex flex-col gap-4 mt-6">
                            <div className="w-full">
                                <div className="flex justify-between items-center mb-2">
                                    <h3 className="text-lg font-semibold">Download</h3>
                                    {fileSize && !error && (
                                        <span className="text-sm text-muted-foreground font-medium bg-muted/50 px-2 py-0.5 rounded">
                                            {fileSize}
                                        </span>
                                    )}
                                </div>

                                <div className="flex flex-col gap-3">
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
                                            disabled={isDownloaded && !isCorrupted}
                                            className={`
                                                relative flex items-center justify-center gap-2 px-6 py-3 rounded-lg font-bold transition-all shadow-md active:scale-95 flex-1
                                                ${isCorrupted ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90' :
                                                    isPaused ? 'bg-yellow-500 text-white hover:bg-yellow-600' :
                                                        isDownloading ? 'bg-muted text-muted-foreground border-2 border-primary/20' :
                                                            isDownloaded ? 'bg-success text-success-foreground' :
                                                                'bg-primary text-primary-foreground hover:bg-primary/90'}
                                            `}
                                            title={isCorrupted ? "File corrupted. Click to retry." : ""}
                                        >
                                            {isDownloading ? (
                                                <>
                                                    <Loader2 className="h-5 w-5 animate-spin" />
                                                    <span className="font-mono">{progress}%</span>
                                                    <Pause className="h-4 w-4 opacity-70 ml-1" />
                                                </>
                                            ) : isPaused ? (
                                                <>
                                                    <Play className="h-5 w-5 fill-current" />
                                                    Resume ({progress}%)
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

                                        {/* Stop/Cancel Button */}
                                        {(isDownloading || isPaused) && (
                                            <button
                                                onClick={cancel}
                                                className="p-3 bg-muted text-muted-foreground rounded-lg hover:bg-destructive hover:text-destructive-foreground transition-colors shadow-sm"
                                                title="Stop and Delete"
                                            >
                                                <Trash2 className="h-5 w-5" />
                                            </button>
                                        )}
                                    </div>

                                    {/* Warnings / Errors */}
                                    {isCorrupted && (
                                        <div className="flex items-center gap-2 text-xs text-destructive font-semibold bg-destructive/10 p-2 rounded">
                                            <AlertTriangle className="h-4 w-4" />
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
                            <div className="group relative flex items-center">
                                <button
                                    onClick={toggleAutoDownload}
                                    className={`
                                        relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2
                                        ${isAutoDownloadEnabled ? 'bg-success' : 'bg-muted'}
                                    `}
                                >
                                    <span className={`
                                        inline-block h-4 w-4 transform rounded-full bg-white transition-transform
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
