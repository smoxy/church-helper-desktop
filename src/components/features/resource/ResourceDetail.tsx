import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';

interface ResourceDetailProps {
    resource: Resource;
    onClose: () => void;
}

export function ResourceDetail({ resource, onClose }: ResourceDetailProps) {
    const { isDownloaded, isDownloading, isAutoDownloadEnabled, fileSize, error, progress, download, toggleAutoDownload } = useResource(resource);

    return (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center p-4 z-50 backdrop-blur-sm" onClick={onClose}>
            <div className="bg-card text-card-foreground rounded-xl shadow-2xl max-w-2xl w-full p-6 animate-in fade-in zoom-in duration-200" onClick={e => e.stopPropagation()}>
                <div className="flex justify-between items-start mb-6">
                    <h2 className="text-2xl font-bold text-primary">{resource.title}</h2>
                    <button onClick={onClose} className="text-muted-foreground hover:text-foreground">
                        <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
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
                                <h3 className="text-lg font-semibold mb-2">Download</h3>
                                <div className="flex flex-wrap items-center gap-3">
                                    <button
                                        onClick={download}
                                        disabled={isDownloaded || isDownloading}
                                        className={`
                                            relative flex items-center gap-3 px-6 py-3 rounded-lg font-bold transition-all shadow-md active:scale-95 whitespace-nowrap flex-1 justify-center overflow-hidden
                                            ${isDownloaded ? 'bg-success text-success-foreground' :
                                                isDownloading ? 'bg-muted text-muted-foreground cursor-wait' :
                                                    'bg-primary text-primary-foreground hover:bg-primary/90'}
                                        `}
                                    >
                                        {isDownloading && progress !== null && (
                                            <div
                                                className="absolute left-0 top-0 bottom-0 bg-primary/10 transition-all duration-300 ease-out"
                                                style={{ width: `${progress}%` }}
                                            />
                                        )}

                                        {isDownloading ? (
                                            <>
                                                {progress !== null ? (
                                                    <span className="relative z-10 font-mono">{progress}%</span>
                                                ) : (
                                                    <>
                                                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="animate-spin"><path d="M21 12a9 9 0 1 1-6.219-8.56"></path></svg>
                                                        Downloading...
                                                    </>
                                                )}
                                            </>
                                        ) : isDownloaded ? (
                                            <>
                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
                                                Downloaded
                                            </>
                                        ) : (
                                            <>
                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="7 10 12 15 17 10"></polyline><line x1="12" y1="15" x2="12" y2="3"></line></svg>
                                                Download Now
                                            </>
                                        )}
                                    </button>

                                    {error && (
                                        <div className="w-full text-xs text-destructive font-medium mt-1 animate-in fade-in slide-in-from-top-1">
                                            {error === "Work directory not configured"
                                                ? "Please set a download folder in Settings."
                                                : error}
                                        </div>
                                    )}

                                    {fileSize && !error && (
                                        <div className="text-sm text-muted-foreground font-medium bg-muted/50 px-3 py-1.5 rounded-md whitespace-nowrap">
                                            {fileSize}
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
