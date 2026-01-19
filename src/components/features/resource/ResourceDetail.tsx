import { Resource } from '../../../types';
import { useResource } from '../../../hooks/useResource';

interface ResourceDetailProps {
    resource: Resource;
    onClose: () => void;
}

export function ResourceDetail({ resource, onClose }: ResourceDetailProps) {
    const { isDownloaded, isDownloading, isAutoDownloadEnabled, download, toggleAutoDownload } = useResource(resource);

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

                        <div className="flex items-center gap-4 mt-6">
                            <div className="flex flex-col items-center">
                                <button
                                    onClick={download}
                                    disabled={isDownloaded || isDownloading}
                                    className={`
                                        w-16 h-16 rounded-full flex items-center justify-center transition-all shadow-lg
                                        ${isDownloaded ? 'bg-success text-success-foreground' :
                                            isDownloading ? 'bg-muted text-muted-foreground animate-pulse' :
                                                'bg-warning text-warning-foreground hover:scale-105 active:scale-95'}
                                    `}
                                    title={isDownloaded ? "Downloaded" : isDownloading ? "Downloading..." : "Click to Download"}
                                >
                                    {isDownloaded ? (
                                        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
                                    ) : isDownloading ? (
                                        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" className="animate-spin"><path d="M21 12a9 9 0 1 1-6.219-8.56"></path></svg>
                                    ) : (
                                        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="7 10 12 15 17 10"></polyline><line x1="12" y1="15" x2="12" y2="3"></line></svg>
                                    )}
                                </button>
                                <span className="text-sm font-medium mt-2 text-muted-foreground">
                                    {isDownloaded ? 'Ready' : isDownloading ? 'Downloading' : 'Download'}
                                </span>
                            </div>

                            <div className="flex-1 bg-secondary/20 p-4 rounded-lg">
                                <div className="flex items-center justify-between mb-2">
                                    <span className="font-medium text-foreground">Auto-download</span>
                                    <button
                                        onClick={toggleAutoDownload}
                                        className={`
                                            relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2
                                            ${isAutoDownloadEnabled ? 'bg-success' : 'bg-muted'}
                                        `}
                                    >
                                        <span className={`
                                            inline-block h-4 w-4 transform rounded-full bg-white transition-transform
                                            ${isAutoDownloadEnabled ? 'translate-x-6' : 'translate-x-1'}
                                        `} />
                                    </button>
                                </div>
                                <p className="text-xs text-muted-foreground">
                                    Automatically download future resources in the <strong>{resource.category}</strong> category.
                                </p>
                            </div>
                        </div>
                    </div>

                    <div className="space-y-4">
                        <div>
                            <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">Category</span>
                            <p className="text-lg font-medium text-foreground capitalize">{resource.category}</p>
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
