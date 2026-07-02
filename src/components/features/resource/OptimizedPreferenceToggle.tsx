import { Switch } from "../../ui/switch";
import { Savings } from "../../../lib/savings";
import { useI18n } from "../../../lib/i18n";

interface OptimizedPreferenceToggleProps {
    savings: Savings | null;
    originalLabel: string | null;
    optimizedLabel: string | null;
    onEnable: () => void;
}

// Presentational-only (UI-dumb guard): no calculation happens here, callers
// pass an already-computed `savings` (see lib/savings.ts::computeSavings).
export function OptimizedPreferenceToggle({ savings, originalLabel, optimizedLabel, onEnable }: OptimizedPreferenceToggleProps) {
    const { t } = useI18n();
    return (
        <div className="flex items-start gap-3 rounded-lg border border-primary/30 bg-primary/5 p-3">
            <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold text-foreground">{t('optimizedToggle.title')}</p>
                {savings ? (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        {t('optimizedToggle.withSavings')}{" "}
                        <span className="font-semibold text-success">−{savings.percent}%</span>
                        {t('optimizedToggle.sizeComparison', { optimizedLabel: optimizedLabel ?? '', originalLabel: originalLabel ?? '' })}
                    </p>
                ) : (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        {t('optimizedToggle.withoutSavings')}
                    </p>
                )}
            </div>
            <Switch
                checked={false}
                onCheckedChange={(c) => { if (c) onEnable(); }}
                aria-label={t('optimizedToggle.ariaLabel')}
                className="mt-0.5 shrink-0"
            />
        </div>
    );
}
