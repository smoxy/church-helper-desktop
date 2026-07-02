import { CircleCheckBig } from "lucide-react";
import { Savings } from "../../../lib/savings";
import { formatBytes } from "../../../lib/utils";
import { useI18n } from "../../../lib/i18n";

interface OptimizedActiveNoticeProps {
    savings: Savings | null;
}

// Presentational-only (UI-dumb guard): mirrors OptimizedPreferenceToggle's
// layout but is shown in its place when prefer_optimized is ALREADY on for a
// resource that has an optimized variant — the callout below only makes
// sense for the OFF case, so without this the user got no feedback at all
// that their setting is already saving them bandwidth on this resource
// (collaudo bug 1: the celebration only fires after a download completes,
// this is the "before download" equivalent, and never overlaps with the
// toggle since callers gate the two on !preferOptimized / preferOptimized).
export function OptimizedActiveNotice({ savings }: OptimizedActiveNoticeProps) {
    const { t } = useI18n();
    return (
        <div className="flex items-start gap-3 rounded-lg border border-success/30 bg-success/5 p-3">
            <CircleCheckBig className="h-5 w-5 text-success shrink-0 mt-0.5" />
            <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold text-foreground">{t('optimizedActive.title')}</p>
                {savings ? (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        {t('optimizedActive.savingsPrefix')}
                        <span className="font-semibold text-success">{formatBytes(savings.savedBytes)}</span>
                        {t('optimizedActive.savingsMiddle')}
                        <span className="font-semibold text-success">{savings.percent}%</span>
                        {t('optimizedActive.savingsSuffix')}
                    </p>
                ) : (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        {t('optimizedActive.withoutSavings')}
                    </p>
                )}
            </div>
        </div>
    );
}
