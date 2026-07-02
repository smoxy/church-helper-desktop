import { Check, Film } from "lucide-react";

import { formatBytes } from "../../../lib/utils";
import { useI18n } from "../../../lib/i18n";
import { OptimizedVideo } from "../../../types";

interface OptimizedVideoPickerProps {
    /** Candidate videos for this resource (adr-0008). Rendered only when
     *  there is an actual choice to make; the caller decides whether to show
     *  this component at all (0/1 elements => don't render it). */
    videos: OptimizedVideo[];
    selectedUrl: string | null;
    onSelect: (url: string) => void;
    /** Disables selection while a download for this resource is in flight. */
    disabled?: boolean;
}

/**
 * Dumb presentational picker: renders the optimized video variants offered
 * by a resource (human label + formatted size) and reports the user's pick
 * via `onSelect`. Holds no state and never calls into Tauri/IPC directly —
 * the selected URL is threaded to the existing download command by
 * `useResource` (adr-0007: download still goes through the queue).
 */
export function OptimizedVideoPicker({ videos, selectedUrl, onSelect, disabled }: OptimizedVideoPickerProps) {
    const { t } = useI18n();
    if (videos.length < 2) return null;

    return (
        <div className="flex flex-col gap-2">
            <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
                {t('optimizedPicker.chooseVideo')}
            </span>
            <div role="radiogroup" aria-label={t('optimizedPicker.ariaLabel')} className="flex flex-col gap-1.5">
                {videos.map((video) => {
                    const isSelected = video.url === selectedUrl;
                    return (
                        <button
                            key={video.url}
                            type="button"
                            role="radio"
                            aria-checked={isSelected}
                            disabled={disabled}
                            onClick={() => onSelect(video.url)}
                            className={`
                                flex items-center justify-between gap-3 px-3 py-2 rounded-lg border text-left transition-colors
                                disabled:opacity-50 disabled:pointer-events-none
                                ${isSelected
                                    ? "border-primary bg-primary/10"
                                    : "border-input bg-background hover:bg-accent hover:text-accent-foreground"}
                            `}
                        >
                            <span className="flex items-center gap-2 min-w-0">
                                <Film className={`h-4 w-4 shrink-0 ${isSelected ? "text-primary" : "text-muted-foreground"}`} />
                                <span className="text-sm font-medium truncate" title={video.label}>{video.label}</span>
                            </span>
                            <span className="flex items-center gap-2 shrink-0">
                                <span className="text-xs text-muted-foreground font-medium bg-muted/50 px-2 py-0.5 rounded">
                                    {formatBytes(video.size_bytes)}
                                </span>
                                {isSelected && <Check className="h-4 w-4 text-primary" />}
                            </span>
                        </button>
                    );
                })}
            </div>
        </div>
    );
}
