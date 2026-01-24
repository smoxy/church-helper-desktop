import { useAppStore } from '../../../stores/appStore';
import { Card, CardContent, CardHeader, CardTitle } from '../../ui/card';
import { Download, Loader2, Pause, Play, Trash2 } from 'lucide-react';

export function DownloadsCard() {
    const { resources, activeDownloads, pauseDownload, resumeDownload, cancelDownload } = useAppStore();

    // Get active downloads with their resource info
    const activeDownloadsList = Object.entries(activeDownloads)
        .filter(([_, download]) => download.status === 'downloading' || download.status === 'paused')
        .map(([id, download]) => {
            const resource = resources.find(r => r.id === Number(id));
            return { id: Number(id), download, resource };
        });

    // Don't show card if no active downloads
    if (activeDownloadsList.length === 0) {
        return null;
    }

    // Calculate average progress
    const totalProgress = activeDownloadsList.length > 0
        ? Math.round(activeDownloadsList.reduce((sum, d) => sum + (d.download.progress || 0), 0) / activeDownloadsList.length)
        : 0;

    const downloadingCount = activeDownloadsList.filter(d => d.download.status === 'downloading').length;
    const pausedCount = activeDownloadsList.filter(d => d.download.status === 'paused').length;

    return (
        <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Active Downloads</CardTitle>
                <Download className="h-4 w-4 text-muted-foreground" />
            </CardHeader>
            <CardContent>
                <div className="text-2xl font-bold flex items-center gap-2">
                    <Loader2 className="h-5 w-5 animate-spin text-primary" />
                    {activeDownloadsList.length}
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                    {downloadingCount > 0 ? `${downloadingCount} downloading` : ''}{pausedCount > 0 ? `${downloadingCount > 0 ? ', ' : ''}${pausedCount} paused` : downloadingCount === 0 ? 'No active' : ''}
                </p>
                
                {/* Mini progress bar */}
                <div className="flex items-center gap-2 mt-3">
                    <div className="flex-1 bg-muted rounded-full h-1.5 overflow-hidden">
                        <div
                            className="h-full bg-primary transition-all duration-300"
                            style={{ width: `${totalProgress}%` }}
                        />
                    </div>
                    <span className="text-xs font-mono text-muted-foreground w-8 text-right">
                        {totalProgress}%
                    </span>
                </div>

                {/* Quick action buttons */}
                {activeDownloadsList.length > 0 && (
                    <div className="flex gap-1 mt-3">
                        {activeDownloadsList.slice(0, 3).map(({ id, download, resource }) => (
                            <div key={id} className="flex gap-0.5">
                                {download.status === 'downloading' && (
                                    <button
                                        onClick={() => pauseDownload(id)}
                                        className="p-1 hover:bg-yellow-500/10 rounded transition-colors"
                                        title={`Pause ${resource?.title || ''}`}
                                    >
                                        <Pause className="h-3 w-3 text-yellow-500" />
                                    </button>
                                )}
                                {download.status === 'paused' && resource && (
                                    <button
                                        onClick={() => resumeDownload(resource)}
                                        className="p-1 hover:bg-primary/10 rounded transition-colors"
                                        title={`Resume ${resource.title}`}
                                    >
                                        <Play className="h-3 w-3 text-primary" />
                                    </button>
                                )}
                                <button
                                    onClick={() => cancelDownload(id)}
                                    className="p-1 hover:bg-destructive/10 rounded transition-colors"
                                    title={`Cancel ${resource?.title || ''}`}
                                >
                                    <Trash2 className="h-3 w-3 text-destructive" />
                                </button>
                            </div>
                        ))}
                    </div>
                )}
            </CardContent>
        </Card>
    );
}
