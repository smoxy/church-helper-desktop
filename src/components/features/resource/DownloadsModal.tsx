import { useAppStore } from "../../../stores/appStore";
import { Button } from "../../ui/button";
import { Play, Pause, X, CloudDownload, CheckCircle2 } from "lucide-react";
import { useEffect, useState, useRef } from "react";

interface DownloadsModalProps {
    open: boolean;
    onClose: () => void;
}

function formatBytes(bytes: number, decimals = 2) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

function formatSpeed(bytesPerSecond: number): string {
    if (bytesPerSecond === 0) return "0 B/s";
    const k = 1024;
    const sizes = ["B/s", "KB/s", "MB/s", "GB/s"];
    const i = Math.floor(Math.log(bytesPerSecond) / Math.log(k));
    return parseFloat((bytesPerSecond / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}

function formatDuration(seconds: number): string {
    if (!isFinite(seconds) || seconds < 0) return "--";
    if (seconds < 60) return `${Math.ceil(seconds)}s`;
    const m = Math.floor(seconds / 60);
    const s = Math.ceil(seconds % 60);
    return `${m}m ${s}s`;
}

export function DownloadsModal({ open, onClose }: DownloadsModalProps) {
    const {
        activeDownloads,
        resources,
        pauseDownload,
        resumeDownload,
        cancelDownload
    } = useAppStore();

    // Local state for speed calculation
    const [downloadStats, setDownloadStats] = useState<Record<number, { speed: number, eta: number, lastBytes: number, lastTime: number }>>({});

    // Use ref to access latest activeDownloads inside interval without resetting it
    const activeDownloadsRef = useRef(activeDownloads);
    useEffect(() => {
        activeDownloadsRef.current = activeDownloads;
    }, [activeDownloads]);

    useEffect(() => {
        if (!open) return;

        const interval = setInterval(() => {
            const now = Date.now();
            setDownloadStats(prev => {
                const newStats = { ...prev };
                const currentDownloads = activeDownloadsRef.current;

                Object.entries(currentDownloads).forEach(([idStr, download]) => {
                    const id = parseInt(idStr);
                    if (download.status !== 'downloading') return;

                    const prevStat = prev[id];
                    const currentBytes = download.currentBytes || 0;

                    if (prevStat) {
                        const timeDiff = (now - prevStat.lastTime) / 1000; // seconds
                        const bytesDiff = currentBytes - prevStat.lastBytes;

                        // Only update if enough time has passed (activeDownloadsRef ensures we have latest data)
                        // Actually, since this interval runs every 1s, timeDiff should be around 1s.

                        if (timeDiff > 0) {
                            const currentSpeed = bytesDiff / timeDiff;
                            // Smooth speed with simple moving average
                            const speed = prevStat.speed === 0 ? currentSpeed : (prevStat.speed * 0.7 + currentSpeed * 0.3);

                            const remainingBytes = (download.totalBytes || 0) - currentBytes;
                            const eta = speed > 0 ? remainingBytes / speed : 0;

                            newStats[id] = {
                                speed,
                                eta,
                                lastBytes: currentBytes,
                                lastTime: now
                            };
                        }
                    } else {
                        // Initialize
                        newStats[id] = {
                            speed: 0,
                            eta: 0,
                            lastBytes: currentBytes,
                            lastTime: now
                        };
                    }
                });
                return newStats;
            });
        }, 1000);

        return () => clearInterval(interval);
    }, [open]);

    if (!open) return null;

    const activeList = Object.entries(activeDownloads)
        .filter(([_, d]) => d.status !== 'completed' && d.status !== 'error')
        .sort((a, b) => {
            // Sort by status (downloading first), then queue position
            if (a[1].status === 'downloading' && b[1].status !== 'downloading') return -1;
            if (a[1].status !== 'downloading' && b[1].status === 'downloading') return 1;
            return (a[1].queuePosition || 999) - (b[1].queuePosition || 999);
        });

    const completedList = Object.entries(activeDownloads)
        .filter(([_, d]) => d.status === 'completed')
        .sort((a, b) => (b[1].startTime || 0) - (a[1].startTime || 0)); // Newest first

    return (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4 z-50 animate-in fade-in duration-200" onClick={onClose}>
            <div className="bg-card text-card-foreground rounded-xl shadow-2xl max-w-2xl w-full max-h-[85vh] flex flex-col overflow-hidden animate-in zoom-in-95 duration-200" onClick={e => e.stopPropagation()}>

                {/* Header */}
                <div className="p-6 border-b flex justify-between items-start">
                    <div>
                        <h2 className="text-xl font-bold">Downloads</h2>
                        <p className="text-sm text-muted-foreground">Manage your active downloads and queue.</p>
                    </div>
                    <button onClick={onClose} className="text-muted-foreground hover:text-foreground transition-colors">
                        <X className="h-6 w-6" />
                    </button>
                </div>

                {/* Content */}
                <div className="flex-1 overflow-y-auto p-6 space-y-6">
                    {/* Active & Queued */}
                    <div className="space-y-3">
                        <h3 className="text-xs font-bold uppercase tracking-widest text-muted-foreground">Active & Queued</h3>

                        {activeList.length === 0 && (
                            <div className="text-center py-12 text-muted-foreground bg-muted/20 rounded-xl border-2 border-dashed">
                                <CloudDownload className="h-10 w-10 mx-auto mb-3 opacity-30" />
                                <p className="text-sm font-medium">No active downloads</p>
                            </div>
                        )}

                        {activeList.map(([idStr, download]) => {
                            const id = parseInt(idStr);
                            const resource = resources.find(r => r.id === id);
                            const stats = downloadStats[id] || { speed: 0, eta: 0 };

                            if (!resource) return null;

                            return (
                                <div key={id} className="bg-muted/30 border rounded-xl p-4 space-y-4 transition-all hover:border-primary/30">
                                    <div className="flex justify-between items-start gap-4">
                                        <div className="min-w-0 flex-1">
                                            <h4 className="font-bold text-base truncate" title={resource.title}>
                                                {resource.title}
                                            </h4>
                                            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1 text-xs text-muted-foreground">
                                                <span className="font-semibold text-primary/80 uppercase tracking-tight">{resource.category}</span>
                                                {download.status === 'downloading' && (
                                                    <>
                                                        <span className="w-1 h-1 rounded-full bg-muted-foreground/30" />
                                                        <span className="font-mono">{formatSpeed(stats.speed)}</span>
                                                        <span className="w-1 h-1 rounded-full bg-muted-foreground/30" />
                                                        <span>ETA: {formatDuration(stats.eta)}</span>
                                                    </>
                                                )}
                                                {download.status === 'pending' && (
                                                    <span className="text-amber-500 font-bold bg-amber-500/10 px-2 py-0.5 rounded-full text-[10px]">
                                                        QUEUED #{download.queuePosition}
                                                    </span>
                                                )}
                                            </div>
                                        </div>

                                        <div className="flex items-center gap-1 shrink-0">
                                            {download.status === 'downloading' ? (
                                                <Button size="icon" variant="ghost" className="h-8 w-8 hover:bg-yellow-500/10 hover:text-yellow-600" onClick={() => pauseDownload(id)}>
                                                    <Pause className="h-4 w-4" />
                                                </Button>
                                            ) : (
                                                <Button size="icon" variant="ghost" className="h-8 w-8 hover:bg-green-500/10 hover:text-green-600" onClick={() => resumeDownload(resource)}>
                                                    <Play className="h-4 w-4" />
                                                </Button>
                                            )}
                                            <Button size="icon" variant="ghost" className="h-8 w-8 text-destructive hover:bg-destructive/10 hover:text-destructive" onClick={() => cancelDownload(id)}>
                                                <X className="h-4 w-4" />
                                            </Button>
                                        </div>
                                    </div>

                                    <div className="space-y-2">
                                        <div className="flex justify-between text-[11px]">
                                            <span className="font-medium text-muted-foreground">
                                                {download.currentBytes ? formatBytes(download.currentBytes) : '0 B'}
                                                {' / '}
                                                {download.totalBytes ? formatBytes(download.totalBytes) : '--'}
                                            </span>
                                            <span className="font-bold text-primary">{Math.round(download.progress)}%</span>
                                        </div>
                                        <div className="w-full bg-muted rounded-full h-2 overflow-hidden shadow-inner">
                                            <div
                                                className={`h-full transition-all duration-500 ease-out ${download.status === 'downloading' ? 'bg-primary' : 'bg-yellow-500'}`}
                                                style={{ width: `${download.progress}%` }}
                                            />
                                        </div>
                                    </div>
                                </div>
                            );
                        })}
                    </div>

                    {/* Recently Completed */}
                    {completedList.length > 0 && (
                        <div className="space-y-3">
                            <h3 className="text-xs font-bold uppercase tracking-widest text-muted-foreground">Recently Completed</h3>
                            <div className="grid gap-2">
                                {completedList.map(([idStr, _download]) => {
                                    const id = parseInt(idStr);
                                    const resource = resources.find(r => r.id === id);
                                    if (!resource) return null;

                                    return (
                                        <div key={id} className="flex items-center gap-4 p-4 rounded-xl border bg-success/5 border-success/20 group">
                                            <div className="h-10 w-10 rounded-full bg-success/10 flex items-center justify-center text-success shrink-0">
                                                <CheckCircle2 className="h-5 w-5" />
                                            </div>
                                            <div className="flex-1 min-w-0">
                                                <h4 className="text-sm font-bold truncate">{resource.title}</h4>
                                                <p className="text-xs text-muted-foreground">Download successfully complete</p>
                                            </div>
                                            <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity" onClick={() => cancelDownload(id)}>
                                                <X className="h-4 w-4" />
                                            </Button>
                                        </div>
                                    );
                                })}
                            </div>
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="p-4 border-t bg-muted/10 text-center">
                    <p className="text-[10px] text-muted-foreground font-medium uppercase tracking-tighter opacity-50">Church Helper Queue System</p>
                </div>
            </div>
        </div>
    );
}
